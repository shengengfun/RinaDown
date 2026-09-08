import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../bindings/bindings.dart';
import '../i18n/locale_provider.dart';
import '../models/settings_provider.dart';
import '../services/file_picker_service.dart';
import '../services/recent_dirs.dart';
import '../services/resolve_preview_client.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';
import 'dir_picker_field.dart';

void showVideoParseDialog(BuildContext context, {String initialUrl = ''}) {
  showShadDialog(
    context: context,
    barrierColor: AppColors.of(context).dialogBarrier,
    animateIn: const [],
    animateOut: const [],
    builder: (_) => _VideoParseDialogContent(initialUrl: initialUrl),
  );
}

class _VideoParseDialogContent extends StatefulWidget {
  /// 预填的视频链接（剪贴板嗅探等外部入口）。
  final String initialUrl;

  const _VideoParseDialogContent({this.initialUrl = ''});

  @override
  State<_VideoParseDialogContent> createState() =>
      _VideoParseDialogContentState();
}

class _VideoParseDialogContentState extends State<_VideoParseDialogContent> {
  final _url = TextEditingController();
  final _saveDir = TextEditingController();
  ResolvePreviewResult? _result;
  ResolvePreviewHandle? _pending;
  int _selected = 0;
  bool _loading = false;
  String _error = '';

  /// 用户是否手动改过保存目录（改过后不再被解析结果自动覆盖）。
  bool _saveDirUserModified = false;

  @override
  void initState() {
    super.initState();
    final u = widget.initialUrl.trim();
    if (u.isNotEmpty) _url.text = u;
  }

  @override
  void dispose() {
    _pending?.cancel();
    _url.dispose();
    _saveDir.dispose();
    super.dispose();
  }

  Future<void> _parse() async {
    final url = _url.text.trim();
    if (url.isEmpty || _loading) return;
    _pending?.cancel();
    setState(() {
      _loading = true;
      _error = '';
      _result = null;
    });
    final handle = ResolvePreviewClient.start(
      url: url,
      cookies: '',
      referrer: '',
      userAgent: '',
      extraHeaders: const {},
      video: true,
    );
    _pending = handle;
    final result = await handle.future;
    if (!mounted || handle.cancelled) return;
    setState(() {
      _loading = false;
      _pending = null;
      _result = result;
      _selected = 0;
      if (result == null) {
        _error = '解析失败，请确认链接可访问，并已安装 yt-dlp 组件。';
      } else {
        // 解析成功：默认目录 = 按文件格式命中的分类目录；无分类命中时
        // 回落到「上次使用/默认」目录（跟随全局记忆设置）。手动改过则保留。
        final settings = SettingsProvider.globalInstance;
        if (!_saveDirUserModified && settings != null) {
          final name = result.fileName;
          final byCategory = settings.resolveCategorySaveDir(name, url: url);
          _saveDir.text = byCategory.isNotEmpty
              ? byCategory
              : settings.effectiveDefaultSaveDir;
        }
      }
    });
  }

  Future<void> _pickDir() async {
    final selected =
        await FilePickerService.pickDirectory(dialogTitle: '选择下载目录');
    if (selected != null && mounted) {
      setState(() {
        _saveDir.text = selected;
        _saveDirUserModified = true;
      });
      SettingsProvider.globalInstance?.recordLastSaveDir(selected);
    }
  }

  void _download() {
    final result = _result;
    final url = _url.text.trim();
    if (result == null || url.isEmpty || result.variants.isEmpty) return;
    final option = result.variants[_selected.clamp(0, result.variants.length - 1)];
    final dir = _saveDir.text.trim();
    if (dir.isNotEmpty) {
      RecentDirs.instance.push(dir);
      // 手动选择的目录记为「上次使用」，供下次新建/解析默认目录使用
      if (_saveDirUserModified) {
        SettingsProvider.globalInstance?.recordLastSaveDir(dir);
      }
    }
    CreateTask(
      url: url,
      saveDir: _saveDir.text.trim(),
      fileName: result.fileName,
      segments: 0,
      cookies: '',
      torrentFileBytes: const [],
      proxyUrl: '',
      userAgent: '',
      queueId: '',
      checksum: '',
      ignoreTlsErrors: false,
      extraHeaders: const {},
      selectedFileIndices: const [],
      startPaused: false,
      httpUser: '',
      httpPassword: '',
      saveSiteAuth: false,
      resolverItem: '__flux_variant:${option.index}',
    ).sendSignalToRust();
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    final s = LocaleScope.of(context);
    final options = _result?.variants ?? const <ResolveVariantOption>[];
    return ShadDialog(
      constraints: const BoxConstraints(maxWidth: 680),
      title: Row(
        children: [
          Icon(LucideIcons.video, color: c.accent, size: 18),
          const SizedBox(width: 8),
          Text(s.videoParse),
        ],
      ),
      description: const Text('解析视频链接并选择下载清晰度'),
      actions: [
        ShadButton.outline(
          onPressed: _loading ? null : () => Navigator.of(context).pop(),
          child: Text(s.cancel),
        ),
        ShadButton(
          onPressed: options.isEmpty || _loading ? null : _download,
          child: const Text('下载', style: TextStyle(color: Color(0xFFFFFFFF))),
        ),
      ],
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: ShadInput(controller: _url, placeholder: const Text('视频链接')),
              ),
              const SizedBox(width: 8),
              ShadButton(onPressed: _loading ? null : _parse, child: Text(_loading ? '解析中…' : '解析')),
            ],
          ),
          const SizedBox(height: 8),
          DirPickerField(
            path: _saveDir.text,
            placeholder: '下载目录',
            enabled: !_loading,
            onTap: _pickDir,
            onPathSelected: (dir) {
              setState(() {
                _saveDir.text = dir;
                _saveDirUserModified = true;
              });
              RecentDirs.instance.push(dir);
              SettingsProvider.globalInstance?.recordLastSaveDir(dir);
            },
          ),
          if (_error.isNotEmpty) ...[
            const SizedBox(height: 10),
            Text(_error, style: TextStyle(color: c.textMuted, fontSize: 12)),
          ],
          if (options.isNotEmpty) ...[
            const SizedBox(height: 14),
            Text(_result?.fileName ?? '', style: TextStyle(color: c.textPrimary, fontWeight: FontWeight.w600)),
            const SizedBox(height: 8),
            ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 300),
              child: SingleChildScrollView(
                child: Column(
                  children: [
                    for (var i = 0; i < options.length; i++)
                      GestureDetector(
                        onTap: () => setState(() => _selected = i),
                        child: Container(
                          margin: const EdgeInsets.only(bottom: 6),
                          padding: const EdgeInsets.all(10),
                          decoration: BoxDecoration(
                            color: i == _selected ? m.subtle(c.accent) : c.surface1,
                            borderRadius: m.brCard,
                            border: Border.all(color: i == _selected ? c.accent : c.border),
                          ),
                          child: Row(
                            children: [
                              Icon(i == _selected ? LucideIcons.circleCheck : LucideIcons.circle, size: 16, color: c.accent),
                              const SizedBox(width: 8),
                              Expanded(child: Text(options[i].label)),
                              if (options[i].totalBytes > 0) Text('${(options[i].totalBytes / 1024 / 1024).toStringAsFixed(1)} MB', style: TextStyle(color: c.textMuted, fontSize: 12)),
                            ],
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
          ],
        ],
      ),
    );
  }
}
