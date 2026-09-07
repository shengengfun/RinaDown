import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../bindings/bindings.dart';
import '../i18n/locale_provider.dart';
import '../models/download_queue.dart';
import '../models/webhook_endpoint.dart';
import '../models/webhook_provider.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';
import 'flux_sonner.dart';
import 'webhook_brand_marks.dart';
import 'webhook_field_theme.dart';
import 'webhook_delivery_panel.dart';

/// 服务预设的品牌标记（预设网格 + 端点行共用）。
///
/// 有官方矢量标的走 [kWebhookBrandPaths] 真 logo，没有的（bark / serverchan
/// 只有位图应用图标，custom 不是品牌）回退字标。
///
/// 只是视觉标识；预设的**行为**（默认模板 / URL 占位符 / Content-Type）
/// 一律来自引擎下发的 [WebhookPresetEntry]，此处不复制。
class WebhookPresetMark {
  const WebhookPresetMark(this.glyph, this.color);

  final String glyph;
  final Color color;

  static const Map<String, WebhookPresetMark> _marks = {
    'ntfy': WebhookPresetMark('n', Color(0xFF34D399)),
    'gotify': WebhookPresetMark('G', Color(0xFF60A5FA)),
    'bark': WebhookPresetMark('B', Color(0xFFF87171)),
    'serverchan': WebhookPresetMark('酱', Color(0xFFFB923C)),
    'telegram': WebhookPresetMark('T', Color(0xFF38BDF8)),
    'discord': WebhookPresetMark('D', Color(0xFF818CF8)),
    'slack': WebhookPresetMark('#', Color(0xFFF472B6)),
    'custom': WebhookPresetMark('{}', Color(0xFFA1A1AA)),
  };

  static WebhookPresetMark of(String presetId) =>
      _marks[presetId] ?? _marks['custom']!;
}

/// 预设标记方块。
class WebhookPresetTile extends StatelessWidget {
  const WebhookPresetTile({super.key, required this.presetId, this.size = 30});

  final String presetId;
  final double size;

  @override
  Widget build(BuildContext context) {
    final mark = WebhookPresetMark.of(presetId);
    final m = AppMetrics.of(context);
    final vector = kWebhookBrandPaths[presetId];
    return Container(
      width: size,
      height: size,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: m.subtle(mark.color),
        borderRadius: m.brMd,
        border: Border.all(color: m.soft(mark.color), width: 1),
      ),
      child: vector == null
          ? Text(
              mark.glyph,
              style: TextStyle(
                fontSize: size * 0.44,
                fontWeight: FontWeight.w600,
                color: mark.color,
              ),
            )
          : CustomPaint(
              size: Size.square(size * 0.56),
              painter: BrandMarkPainter(
                path: parseBrandPath(vector),
                color: mark.color,
              ),
            ),
    );
  }
}

/// 打开「添加 / 编辑 Webhook 端点」对话框。
///
/// 返回保存后的端点；用户取消返回 `null`。**测试投递不需要先保存**——
/// 页脚「发送测试」直接把当前草稿发给引擎。
Future<WebhookEndpoint?> showWebhookEndpointDialog({
  required BuildContext context,
  required WebhookProvider webhook,
  required List<DownloadQueue> queues,
  WebhookEndpoint? initial,
}) {
  return showShadDialog<WebhookEndpoint>(
    context: context,
    barrierColor: AppColors.of(context).dialogBarrier,
    animateIn: const [],
    animateOut: const [],
    builder: (_) => _WebhookEndpointDialog(
      webhook: webhook,
      queues: queues,
      initial: initial,
    ),
  );
}

class _WebhookEndpointDialog extends StatefulWidget {
  const _WebhookEndpointDialog({
    required this.webhook,
    required this.queues,
    this.initial,
  });

  final WebhookProvider webhook;
  final List<DownloadQueue> queues;
  final WebhookEndpoint? initial;

  @override
  State<_WebhookEndpointDialog> createState() => _WebhookEndpointDialogState();
}

class _WebhookEndpointDialogState extends State<_WebhookEndpointDialog> {
  late final TextEditingController _nameCtrl;
  late final TextEditingController _urlCtrl;
  late final TextEditingController _templateCtrl;
  late final TextEditingController _secretCtrl;
  final FocusNode _templateFocus = FocusNode();
  final FocusNode _urlFocus = FocusNode();

  late String _preset;
  late Set<String> _events;
  late String _queueId;
  late List<_HeaderRow> _headers;
  late bool _signEnabled;
  late bool _allowHttp;
  late bool _useProxy;

  bool _advancedOpen = false;
  bool _urlTouched = false;

  @override
  void initState() {
    super.initState();
    final e = widget.initial;
    _nameCtrl = TextEditingController(text: e?.name ?? '');
    _urlCtrl = TextEditingController(text: e?.url ?? '');
    _templateCtrl = TextEditingController(text: e?.bodyTemplate ?? '');
    _secretCtrl = TextEditingController(text: e?.signSecret ?? '');
    _preset = e?.preset ?? WebhookEndpoint.kPresetCustom;
    _events = {...(e?.events ?? WebhookEvents.defaults)};
    _queueId = e?.queueId ?? '';
    _headers = (e?.headers ?? const <String, String>{}).entries
        .map((kv) => _HeaderRow(kv.key, kv.value))
        .toList();
    _signEnabled = (e?.signSecret ?? '').isNotEmpty;
    _allowHttp = e?.allowHttp ?? false;
    _useProxy = e?.useProxy ?? false;
    // 已有自定义头/模板/签名的端点直接展开高级区——否则用户会以为配置丢了。
    _advancedOpen =
        _headers.isNotEmpty ||
        _templateCtrl.text.isNotEmpty ||
        _signEnabled ||
        _allowHttp ||
        _useProxy;
    // 失焦才亮红字：边打字边报错是噪音，不是帮助。
    _urlFocus.addListener(() {
      if (!_urlFocus.hasFocus && mounted) setState(() => _urlTouched = true);
    });
    // `initState` 在 build 相位里跑：通知监听者 = 构建途中 markNeedsBuild。
    widget.webhook.resetTestState(notify: false);
  }

  @override
  void dispose() {
    _nameCtrl.dispose();
    _urlCtrl.dispose();
    _templateCtrl.dispose();
    _secretCtrl.dispose();
    _templateFocus.dispose();
    _urlFocus.dispose();
    for (final h in _headers) {
      h.dispose();
    }
    super.dispose();
  }

  // ---------------------------------------------------------------------------
  // 草稿 → 模型
  // ---------------------------------------------------------------------------

  WebhookEndpoint _draft() {
    final headers = <String, String>{};
    for (final h in _headers) {
      final key = h.nameCtrl.text.trim();
      if (key.isEmpty) continue;
      headers[key] = h.valueCtrl.text;
    }
    return WebhookEndpoint(
      id: widget.initial?.id ?? generateWebhookEndpointId(),
      name: _nameCtrl.text.trim(),
      preset: _preset,
      url: _urlCtrl.text.trim(),
      enabled: widget.initial?.enabled ?? true,
      events: WebhookEvents.all.where(_events.contains).toList(),
      queueId: _queueId,
      headers: headers,
      bodyTemplate: _templateCtrl.text,
      signSecret: _signEnabled ? _secretCtrl.text.trim() : '',
      allowHttp: _allowHttp,
      useProxy: _useProxy,
    );
  }

  /// URL 内联校验：空 / 非 http(s) / 明文未放行。返回 null = 通过。
  String? _urlError(S s) {
    final raw = _urlCtrl.text.trim();
    if (raw.isEmpty) return null; // 空 URL 由保存按钮禁用兜底，不红框吓人
    final uri = Uri.tryParse(raw);
    if (uri == null || !uri.hasScheme || uri.host.isEmpty) {
      return s.webhookUrlInvalid;
    }
    if (uri.scheme == 'http' && !_allowHttp) return s.webhookUrlWarnHttp;
    if (uri.scheme != 'http' && uri.scheme != 'https') {
      return s.webhookUrlInvalid;
    }
    return null;
  }

  bool get _canSave =>
      _nameCtrl.text.trim().isNotEmpty &&
      _urlCtrl.text.trim().isNotEmpty &&
      _urlError(LocaleScope.of(context)) == null;

  void _save() {
    if (!_canSave) return;
    Navigator.of(context).pop(_draft());
  }

  void _insertVariable(String variable) {
    final selection = _templateCtrl.selection;
    final text = _templateCtrl.text;
    final start = selection.start < 0 ? text.length : selection.start;
    final end = selection.end < 0 ? text.length : selection.end;
    final next = text.replaceRange(start, end, variable);
    _templateCtrl.value = TextEditingValue(
      text: next,
      selection: TextSelection.collapsed(offset: start + variable.length),
    );
    _templateFocus.requestFocus();
    setState(() {});
  }

  // ---------------------------------------------------------------------------
  // 实时载荷预览
  // ---------------------------------------------------------------------------

  /// 样例变量表——与引擎 `WebhookEvent::sample()` 对齐，只用于预览。
  static const Map<String, String> _sampleVars = {
    '{event}': 'task.completed',
    '{event.title}': 'Download completed',
    '{event.summary}': 'ubuntu-24.04.2-desktop-amd64.iso · 6.0 GB',
    '{timestamp}': '2026-07-17T12:34:56Z',
    '{instance.app}': 'fluxdown',
    '{instance.version}': '0.1.44',
    '{instance.host}': 'DESKTOP',
    '{task.id}': '00000000-0000-4000-8000-000000000000',
    '{task.fileName}': 'ubuntu-24.04.2-desktop-amd64.iso',
    '{task.url}': 'https://releases.ubuntu.com/24.04/ubuntu.iso',
    '{task.saveDir}': '/downloads',
    '{task.totalBytes}': '6442450944',
    '{task.totalBytesHuman}': '6.0 GB',
    '{task.status}': '3',
    '{task.errorMessage}': '',
    '{queue.id}': 'main',
    '{queue.name}': 'Main',
    '{ntfy.topic}': 'my-topic',
  };

  /// 占位符替换——与引擎 `render_template` 同规则：一个占位符是**不含嵌套
  /// `{`** 的 `{…}` 段，未知段原样保留，因此 JSON 字面量不会被破坏。
  static String _renderPreview(String template, bool formEscape) {
    final out = StringBuffer();
    var i = 0;
    while (i < template.length) {
      if (template[i] != '{') {
        final start = i;
        while (i < template.length && template[i] != '{') {
          i++;
        }
        out.write(template.substring(start, i));
        continue;
      }
      var j = i + 1;
      while (j < template.length && template[j] != '}' && template[j] != '{') {
        j++;
      }
      if (j >= template.length || template[j] == '{') {
        out.write('{');
        i++;
        continue;
      }
      final key = template.substring(i, j + 1);
      final value = _sampleVars[key];
      if (value == null) {
        out.write(key);
      } else if (formEscape) {
        out.write(Uri.encodeQueryComponent(value));
      } else {
        // JSON 字符串上下文：借 jsonEncode 转义后剥掉外层引号。
        final encoded = jsonEncode(value);
        out.write(encoded.substring(1, encoded.length - 1));
      }
      i = j + 1;
    }
    return out.toString();
  }

  String _previewBody(WebhookPresetEntry? preset) {
    final template = _templateCtrl.text.trim().isNotEmpty
        ? _templateCtrl.text
        : (preset?.defaultTemplate ?? '');
    if (template.isEmpty) {
      // custom 预设无模板 → §3.2 信封原文。
      return const JsonEncoder.withIndent('  ').convert({
        'schemaVersion': 1,
        'event': _sampleVars['{event}'],
        'deliveryId': '5f2a91c7-8b3e-4d10-a6f4-c2d90b7e13aa',
        'timestamp': _sampleVars['{timestamp}'],
        'instance': {
          'app': _sampleVars['{instance.app}'],
          'version': _sampleVars['{instance.version}'],
          'host': _sampleVars['{instance.host}'],
        },
        'queue': {
          'id': _sampleVars['{queue.id}'],
          'name': _sampleVars['{queue.name}'],
        },
        'task': {
          'id': _sampleVars['{task.id}'],
          'fileName': _sampleVars['{task.fileName}'],
          'url': _sampleVars['{task.url}'],
          'saveDir': _sampleVars['{task.saveDir}'],
          'totalBytes': 6442450944,
          'status': 3,
          'errorMessage': '',
        },
      });
    }
    final isForm = (preset?.contentType ?? '').startsWith(
      'application/x-www-form',
    );
    final rendered = _renderPreview(template, isForm);
    if (isForm) return rendered;
    // 渲染结果若是合法 JSON 就美化一下，方便扫读；不是就原样显示。
    try {
      return const JsonEncoder.withIndent('  ').convert(jsonDecode(rendered));
    } catch (_) {
      return rendered;
    }
  }

  // ---------------------------------------------------------------------------
  // build
  // ---------------------------------------------------------------------------

  @override
  Widget build(BuildContext context) {
    final s = LocaleScope.of(context);
    final c = AppColors.of(context);
    final preset = widget.webhook.presetById(_preset);

    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.enter, control: true): _save,
        const SingleActivator(LogicalKeyboardKey.enter, meta: true): _save,
      },
      child: WebhookFieldTheme(
        child: ShadDialog(
          // 默认 maxWidth 512 装不下双栏。**不给内容写死宽度**——`ShadDialog`
          // 的内边距由主题决定，写死就会差几个像素溢出；让 Row 自己填满可用
          // 宽度，任何主题下都不会溢出。
          constraints: const BoxConstraints(maxWidth: 900),
          title: Text(
            widget.initial == null
                ? s.webhookDialogAddTitle
                : s.webhookDialogEditTitle,
          ),
          description: Text(s.webhookDialogDesc),
          // 页脚放进 child 而不是 `actions`：`actions` 走
          // `Flex(mainAxisSize: min)`，子项拿到的是**无界主轴约束**，里面的
          // `Spacer`/`Flexible` 会抛 RenderFlex 断言并打断整帧（连带把
          // MouseTracker 卡在 device-update 相位，之后每个指针事件都断言，
          // 对话框根本不出现）。放进 child 就是有界的普通 Row。
          actions: const [],
          child: SizedBox(
            height: 512,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Expanded(
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Expanded(
                        child: SingleChildScrollView(
                          // 左侧留出焦点环的余量：环画在边框外面，贴着视口边
                          // 就会被裁掉，看起来像输入框"边框超出宽度"。
                          padding: const EdgeInsets.only(
                            left: kWebhookFocusRingSlack,
                            right: 14,
                            top: 6,
                            bottom: 6,
                          ),
                          child: _buildForm(s, c, preset),
                        ),
                      ),
                      Container(
                        width: 1,
                        color: AppMetrics.of(context).borderFade(c.border),
                      ),
                      SizedBox(width: 300, child: _buildPreview(s, c, preset)),
                    ],
                  ),
                ),
                const SizedBox(height: 12),
                Container(
                  height: 1,
                  color: AppMetrics.of(context).borderFade(c.border),
                ),
                const SizedBox(height: 12),
                _buildFooter(s, c),
              ],
            ),
          ),
        ),
      ),
    );
  }

  // ---- 左栏：表单（自上而下 = 用户决策顺序）----

  Widget _buildForm(S s, AppColors c, WebhookPresetEntry? preset) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _label(s.webhookFieldPreset, c),
        const SizedBox(height: 8),
        _buildPresetGrid(c),
        const SizedBox(height: 16),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              width: 170,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _label(s.webhookFieldName, c),
                  const SizedBox(height: 6),
                  ShadInput(
                    controller: _nameCtrl,
                    placeholder: Text(preset?.label ?? ''),
                    onChanged: (_) => setState(() {}),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 12),
            Expanded(child: _buildUrlField(s, c, preset)),
          ],
        ),
        const SizedBox(height: 16),
        _label(s.webhookFieldEvents, c),
        const SizedBox(height: 6),
        Text(
          _events.isEmpty ? s.webhookEventsEmpty : s.webhookEventsHint,
          style: TextStyle(
            fontSize: 11,
            color: _events.isEmpty ? c.statusWarning : c.textMuted,
          ),
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: [
            for (final event in WebhookEvents.all)
              _Chip(
                label: s.webhookEventLabel(event),
                selected: _events.contains(event),
                onTap: () => setState(() {
                  if (!_events.remove(event)) _events.add(event);
                }),
              ),
          ],
        ),
        const SizedBox(height: 16),
        _label(s.webhookFieldQueue, c),
        const SizedBox(height: 6),
        SizedBox(
          width: 220,
          child: ShadSelect<String>(
            initialValue: _queueId,
            selectedOptionBuilder: (context, value) =>
                Text(_queueName(s, value)),
            options: [
              ShadOption(value: '', child: Text(s.webhookQueueAll)),
              for (final q in widget.queues)
                ShadOption(value: q.queueId, child: Text(q.name)),
            ],
            onChanged: (v) => setState(() => _queueId = v ?? ''),
          ),
        ),
        const SizedBox(height: 14),
        _buildAdvanced(s, c),
      ],
    );
  }

  String _queueName(S s, String id) {
    if (id.isEmpty) return s.webhookQueueAll;
    for (final q in widget.queues) {
      if (q.queueId == id) return q.name;
    }
    return id;
  }

  Widget _buildUrlField(S s, AppColors c, WebhookPresetEntry? preset) {
    final error = _urlTouched ? _urlError(s) : null;
    final hint = _preset == 'ntfy' ? s.webhookUrlHintNtfy : s.webhookUrlHint;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _label(s.webhookFieldUrl, c),
        const SizedBox(height: 6),
        ShadInput(
          controller: _urlCtrl,
          placeholder: Text(preset?.urlPlaceholder ?? ''),
          focusNode: _urlFocus,
          onChanged: (_) => setState(() {}),
          onSubmitted: (_) => setState(() => _urlTouched = true),
        ),
        const SizedBox(height: 4),
        Text(
          error ?? hint,
          style: TextStyle(
            fontSize: 10.5,
            color: error == null ? c.textMuted : c.statusWarning,
          ),
        ),
      ],
    );
  }

  Widget _buildPresetGrid(AppColors c) {
    final presets = widget.webhook.presets;
    if (presets.isEmpty) {
      // 预设目录尚未回流（打开设置页即请求，通常瞬时）。
      return const SizedBox(height: 4);
    }
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      children: [
        for (final p in presets)
          _PresetTileButton(
            preset: p,
            selected: p.id == _preset,
            onTap: () => setState(() => _preset = p.id),
          ),
      ],
    );
  }

  // ---- 高级区（渐进披露）----

  Widget _buildAdvanced(S s, AppColors c) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        MouseRegion(
          cursor: SystemMouseCursors.click,
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () => setState(() => _advancedOpen = !_advancedOpen),
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: Row(
                children: [
                  AnimatedRotation(
                    turns: _advancedOpen ? 0.25 : 0,
                    duration: const Duration(milliseconds: 160),
                    child: Icon(
                      LucideIcons.chevronRight,
                      size: 14,
                      color: c.textSecondary,
                    ),
                  ),
                  const SizedBox(width: 6),
                  Text(
                    s.webhookAdvanced,
                    style: TextStyle(fontSize: 12, color: c.textSecondary),
                  ),
                ],
              ),
            ),
          ),
        ),
        AnimatedSize(
          duration: const Duration(milliseconds: 160),
          alignment: Alignment.topCenter,
          child: _advancedOpen
              ? Padding(
                  padding: const EdgeInsets.only(top: 10),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      _buildHeadersEditor(s, c),
                      const SizedBox(height: 16),
                      _buildTemplateEditor(s, c),
                      const SizedBox(height: 16),
                      _switchRow(
                        c,
                        title: s.webhookFieldSign,
                        desc: s.webhookSignDesc,
                        value: _signEnabled,
                        onChanged: (v) => setState(() {
                          _signEnabled = v;
                          if (v && _secretCtrl.text.trim().isEmpty) {
                            _secretCtrl.text = generateWebhookSecret();
                          }
                        }),
                      ),
                      if (_signEnabled) ...[
                        const SizedBox(height: 8),
                        Row(
                          children: [
                            Expanded(
                              child: ShadInput(
                                controller: _secretCtrl,
                                onChanged: (_) => setState(() {}),
                              ),
                            ),
                            const SizedBox(width: 8),
                            ShadButton.outline(
                              size: ShadButtonSize.sm,
                              onPressed: () => setState(
                                () =>
                                    _secretCtrl.text = generateWebhookSecret(),
                              ),
                              child: Text(s.webhookRegenerate),
                            ),
                            const SizedBox(width: 6),
                            ShadButton.outline(
                              size: ShadButtonSize.sm,
                              onPressed: () => _copy(_secretCtrl.text, s),
                              child: Text(s.webhookCopy),
                            ),
                          ],
                        ),
                      ],
                      const SizedBox(height: 14),
                      _switchRow(
                        c,
                        title: s.webhookFieldAllowHttp,
                        desc: s.webhookAllowHttpDesc,
                        value: _allowHttp,
                        onChanged: (v) => setState(() => _allowHttp = v),
                      ),
                      const SizedBox(height: 14),
                      _switchRow(
                        c,
                        title: s.webhookFieldUseProxy,
                        desc: s.webhookUseProxyDesc,
                        value: _useProxy,
                        onChanged: (v) => setState(() => _useProxy = v),
                      ),
                    ],
                  ),
                )
              : const SizedBox(width: double.infinity),
        ),
      ],
    );
  }

  Widget _buildHeadersEditor(S s, AppColors c) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _label(s.webhookFieldHeaders, c),
        const SizedBox(height: 6),
        for (var i = 0; i < _headers.length; i++)
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Row(
              children: [
                SizedBox(
                  width: 150,
                  child: ShadInput(
                    placeholder: Text(s.webhookHeaderName),
                    controller: _headers[i].nameCtrl,
                    onChanged: (_) => setState(() {}),
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: ShadInput(
                    placeholder: Text(s.webhookHeaderValue),
                    controller: _headers[i].valueCtrl,
                    onChanged: (_) => setState(() {}),
                  ),
                ),
                const SizedBox(width: 6),
                ShadButton.ghost(
                  size: ShadButtonSize.sm,
                  onPressed: () =>
                      setState(() => _headers.removeAt(i).dispose()),
                  child: Icon(LucideIcons.x, size: 13, color: c.textMuted),
                ),
              ],
            ),
          ),
        Align(
          alignment: Alignment.centerLeft,
          child: ShadButton.outline(
            size: ShadButtonSize.sm,
            onPressed: () => setState(() => _headers.add(_HeaderRow('', ''))),
            child: Text(s.webhookAddHeader),
          ),
        ),
      ],
    );
  }

  Widget _buildTemplateEditor(S s, AppColors c) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _label(s.webhookFieldTemplate, c),
        const SizedBox(height: 6),
        ShadInput(
          controller: _templateCtrl,
          focusNode: _templateFocus,
          maxLines: 4,
          minLines: 3,
          placeholder: Text(s.webhookTemplatePlaceholder),
          style: const TextStyle(fontFamily: 'monospace', fontSize: 11.5),
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 4),
        Text(
          s.webhookTemplateHint,
          style: TextStyle(fontSize: 10.5, color: c.textMuted),
        ),
        const SizedBox(height: 6),
        Wrap(
          spacing: 5,
          runSpacing: 5,
          children: [
            for (final variable in widget.webhook.variables)
              _VariableChip(
                label: variable,
                onTap: () => _insertVariable(variable),
              ),
          ],
        ),
      ],
    );
  }

  // ---- 右栏：实时载荷预览 ----

  Widget _buildPreview(S s, AppColors c, WebhookPresetEntry? preset) {
    final m = AppMetrics.of(context);
    final url = _urlCtrl.text.trim().isEmpty
        ? (preset?.urlPlaceholder ?? '')
        : _urlCtrl.text.trim();
    final firstEvent = WebhookEvents.all.firstWhere(
      _events.contains,
      orElse: () => WebhookEvents.taskCompleted,
    );
    final headLines = <String>[
      'POST $url',
      'Content-Type: ${preset?.contentType ?? 'application/json'}',
      'X-FluxDown-Event: $firstEvent',
      'X-FluxDown-Delivery: 5f2a91c7-…',
      if (_signEnabled) 'X-FluxDown-Signature: t=1789647128,v1=9c41f2…',
    ];
    return Padding(
      padding: const EdgeInsets.only(left: 14, top: 6, bottom: 6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            s.webhookPreviewTitle,
            style: TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w600,
              color: c.textSecondary,
            ),
          ),
          const SizedBox(height: 8),
          Expanded(
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
              decoration: BoxDecoration(
                color: c.bg,
                borderRadius: m.brMd,
                border: Border.all(color: m.borderMedium(c.border), width: 1),
              ),
              child: SingleChildScrollView(
                child: SelectableMonoText(
                  text:
                      '${headLines.join('\n')}\n${'─' * 28}\n${_previewBody(preset)}',
                  fontSize: 10.8,
                  height: 1.6,
                ),
              ),
            ),
          ),
          const SizedBox(height: 8),
          Text(
            s.webhookPreviewMeta,
            style: TextStyle(fontSize: 10.5, height: 1.7, color: c.textMuted),
          ),
        ],
      ),
    );
  }

  // ---- 页脚：发送测试（内联反馈）+ 取消 / 保存 ----

  /// 由 `child` 的 Column 承载，宽度有界，`Spacer`/`Flexible` 可以放心用。
  Widget _buildFooter(S s, AppColors c) {
    return ListenableBuilder(
      listenable: widget.webhook,
      builder: (context, _) {
        final result = widget.webhook.lastTestResult;
        final testing = widget.webhook.testing;
        return Row(
          children: [
            ShadButton.outline(
              size: ShadButtonSize.sm,
              enabled: !testing && _urlCtrl.text.trim().isNotEmpty,
              onPressed: () => widget.webhook.testEndpoint(_draft()),
              child: testing
                  // 真发一次 HTTP（超时 10s）：没有转圈用户只会再点一次。
                  ? Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        WebhookSpinner(color: c.textMuted, size: 10),
                        const SizedBox(width: 6),
                        Text(s.webhookTesting),
                      ],
                    )
                  : Text(s.webhookSendTest),
            ),
            const SizedBox(width: 10),
            if (result != null)
              Flexible(
                child: Text(
                  result.success
                      ? s.webhookTestOk(
                          result.statusCode == 0
                              ? 'OK'
                              : result.statusCode.toString(),
                          result.latencyMs,
                        )
                      : s.webhookTestFail(
                          result.errorMessage.isEmpty
                              ? result.statusCode.toString()
                              : result.errorMessage,
                        ),
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: result.success ? c.statusSuccess : c.statusError,
                  ),
                ),
              ),
            const Spacer(),
            ShadButton.ghost(
              size: ShadButtonSize.sm,
              onPressed: () => Navigator.of(context).pop(),
              child: Text(s.cancel),
            ),
            const SizedBox(width: 8),
            ShadButton(
              size: ShadButtonSize.sm,
              enabled: _canSave,
              onPressed: _save,
              child: Text(s.webhookSaveEndpoint),
            ),
          ],
        );
      },
    );
  }

  // ---- 小零件 ----

  Widget _label(String text, AppColors c) => Text(
    text,
    style: TextStyle(
      fontSize: 11.5,
      fontWeight: FontWeight.w500,
      color: c.textSecondary,
    ),
  );

  Widget _switchRow(
    AppColors c, {
    required String title,
    required String desc,
    required bool value,
    required ValueChanged<bool> onChanged,
  }) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                title,
                style: TextStyle(fontSize: 12.5, color: c.textPrimary),
              ),
              const SizedBox(height: 2),
              Text(
                desc,
                style: TextStyle(fontSize: 11, height: 1.5, color: c.textMuted),
              ),
            ],
          ),
        ),
        const SizedBox(width: 16),
        ShadSwitch(value: value, onChanged: onChanged),
      ],
    );
  }

  Future<void> _copy(String text, S s) async {
    await Clipboard.setData(ClipboardData(text: text));
    if (!mounted) return;
    FluxSonner.of(context).show(
      ShadToast(
        title: Text(s.webhookCopied),
        duration: const Duration(seconds: 2),
      ),
    );
  }
}

/// 一行自定义请求头。用 controller 而非裸 String：行可被删除，
/// `ShadInput.initialValue` 在列表重排后会把值串到错误的行上。
class _HeaderRow {
  _HeaderRow(String name, String value)
    : nameCtrl = TextEditingController(text: name),
      valueCtrl = TextEditingController(text: value);

  final TextEditingController nameCtrl;
  final TextEditingController valueCtrl;

  void dispose() {
    nameCtrl.dispose();
    valueCtrl.dispose();
  }
}

/// 预设网格里的一个 tile。
class _PresetTileButton extends StatelessWidget {
  const _PresetTileButton({
    required this.preset,
    required this.selected,
    required this.onTap,
  });

  final WebhookPresetEntry preset;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          width: 82,
          padding: const EdgeInsets.symmetric(vertical: 8),
          decoration: BoxDecoration(
            color: selected ? m.subtle(c.accent) : c.surface2,
            borderRadius: m.brMd,
            border: Border.all(
              color: selected ? c.accent : m.borderMedium(c.border),
              width: 1,
            ),
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              WebhookPresetTile(presetId: preset.id, size: 26),
              const SizedBox(height: 6),
              Text(
                preset.label,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 11,
                  color: selected ? c.textPrimary : c.textSecondary,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 事件订阅芯片。
class _Chip extends StatelessWidget {
  const _Chip({
    required this.label,
    required this.selected,
    required this.onTap,
  });

  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 11, vertical: 4),
          decoration: BoxDecoration(
            color: selected ? c.accent : c.surface2,
            borderRadius: BorderRadius.circular(999),
            border: Border.all(
              color: selected ? c.accent : m.borderMedium(c.border),
              width: 1,
            ),
          ),
          child: Text(
            label,
            style: TextStyle(
              fontSize: 11.5,
              fontWeight: selected ? FontWeight.w500 : FontWeight.w400,
              color: selected ? c.accentForeground : c.textSecondary,
            ),
          ),
        ),
      ),
    );
  }
}

/// 「点击插入」的变量芯片。
class _VariableChip extends StatelessWidget {
  const _VariableChip({required this.label, required this.onTap});

  final String label;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
          decoration: BoxDecoration(
            borderRadius: m.brSm,
            border: Border.all(color: m.borderMedium(c.border), width: 1),
          ),
          child: Text(
            label,
            style: TextStyle(
              fontFamily: 'monospace',
              fontSize: 10,
              color: c.textMuted,
            ),
          ),
        ),
      ),
    );
  }
}
