import 'package:flutter/material.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../bindings/bindings.dart';
import '../i18n/locale_provider.dart';
import '../models/webhook_endpoint.dart';
import '../models/webhook_provider.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';

/// 打开推送记录抽屉（右侧滑出）。
///
/// 记录是「为什么没收到」的唯一产品内答案——设计的核心论点是：
/// 没有推送记录的 webhook 等于没有 webhook。
///
/// [endpoints] 用来把记录里的目标 ID 翻成名字，并驱动顶部的筛选下拉；
/// [endpointId] 非空则打开时就只看那一个（行内「日志」入口）。
///
/// **不在这里拉记录**：拉取是一条 rinf 信号（副作用），塞进「打开一个弹层」
/// 里会让这个函数在无原生 runtime 的 widget test 里直接抛。由调用方在打开
/// 前自行 `webhook.refresh()`。
Future<void> showWebhookDeliveryPanel({
  required BuildContext context,
  required WebhookProvider webhook,
  List<WebhookEndpoint> endpoints = const [],
  String endpointId = '',
}) {
  return showShadSheet<void>(
    context: context,
    side: ShadSheetSide.right,
    barrierColor: AppColors.of(context).dialogBarrier,
    builder: (_) => _WebhookDeliveryPanel(
      webhook: webhook,
      endpoints: endpoints,
      endpointId: endpointId,
    ),
  );
}

class _WebhookDeliveryPanel extends StatefulWidget {
  const _WebhookDeliveryPanel({
    required this.webhook,
    required this.endpoints,
    required this.endpointId,
  });

  final WebhookProvider webhook;
  final List<WebhookEndpoint> endpoints;
  final String endpointId;

  @override
  State<_WebhookDeliveryPanel> createState() => _WebhookDeliveryPanelState();
}

class _WebhookDeliveryPanelState extends State<_WebhookDeliveryPanel> {
  /// 展开详情的 deliveryId 集合。
  final Set<String> _expanded = {};

  /// 目标过滤（空 = 全部）。初值来自入口，用户可在面板里改。
  late String _filterId = widget.endpointId;

  /// 目标 ID → 展示名。记录里存了名字快照，但端点改名后旧记录会显示旧名；
  /// 以当前端点表为准，查不到（已删）才回落记录里的名字，再回落 ID。
  String _nameOf(String id, List<WebhookDeliveryEntry> log) {
    for (final e in widget.endpoints) {
      if (e.id == id) return e.name.isEmpty ? e.url : e.name;
    }
    for (final d in log) {
      if (d.endpointId == id && d.endpointName.isNotEmpty) {
        return d.endpointName;
      }
    }
    return id;
  }

  @override
  Widget build(BuildContext context) {
    final s = LocaleScope.of(context);
    final c = AppColors.of(context);
    return ShadSheet(
      // 不给内容写死宽度：内边距由主题决定，写死就差几像素溢出。
      constraints: const BoxConstraints(maxWidth: 560),
      // 关掉外层滚动，`child` 槽位才拿得到**有界**高度；否则它在
      // `SingleChildScrollView` 里是无界的，只能写死高度 —— 而写死的那个数
      // 在别的窗口高度下必然对不上（页脚浮在半空或被切掉一截）。列表自己
      // 在里面滚。
      scrollable: false,
      title: Text(s.webhookDeliveryLog),
      description: Text(s.webhookLogSubtitle),
      // 页脚放进 child：`actions` 走 `Flex(mainAxisSize: min)`，子项拿到无界
      // 主轴约束，`Spacer` 会抛 RenderFlex 断言并打断整帧。
      actions: const [],
      child: LayoutBuilder(
        builder: (context, box) {
          // 标题/描述/内边距占多少由主题和文案换行决定，这里不猜：直接吃掉
          // 剩下的全部空间，页脚就精确贴在抽屉底部。
          return SizedBox(
            width: box.maxWidth,
            height: box.maxHeight,
            child: ListenableBuilder(
              listenable: widget.webhook,
              builder: (context, _) {
                final all = widget.webhook.deliveries;
                final pending = widget.webhook.pendingCount;
                final rows = _filterId.isEmpty
                    ? all
                    : all.where((d) => d.endpointId == _filterId).toList();
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    if (widget.endpoints.isNotEmpty) _buildFilterBar(s, c, all),
                    // 在途投递排在最前：请求真在飞（超时 10s + 重试 3 次），
                    // 空列表配一个转不动的按钮只会让人以为点坏了。
                    if (pending > 0) _buildPendingRow(s, c, pending),
                    Expanded(
                      child: rows.isEmpty
                          ? (pending > 0
                                ? const SizedBox.shrink()
                                : Center(
                                    child: Text(
                                      s.webhookLogEmpty,
                                      style: TextStyle(
                                        fontSize: 12,
                                        color: c.textMuted,
                                      ),
                                    ),
                                  ))
                          : ListView.builder(
                              padding: EdgeInsets.zero,
                              itemCount: rows.length,
                              itemBuilder: (context, i) =>
                                  _buildRow(s, c, rows[i]),
                            ),
                    ),
                    const SizedBox(height: 10),
                    Container(
                      height: 1,
                      color: AppMetrics.of(context).borderFade(c.border),
                    ),
                    const SizedBox(height: 10),
                    _buildFooter(s),
                  ],
                );
              },
            ),
          );
        },
      ),
    );
  }

  /// 目标筛选：常驻下拉，「全部目标」+ 每个已配置目标。行内「日志」入口
  /// 进来时预选那一条，用户可以就地换看别的，不用退出去重开。
  Widget _buildFilterBar(S s, AppColors c, List<WebhookDeliveryEntry> all) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Row(
        children: [
          Text(
            s.webhookLogFilterLabel,
            style: TextStyle(fontSize: 11.5, color: c.textMuted),
          ),
          const SizedBox(width: 8),
          SizedBox(
            width: 220,
            child: ShadSelect<String>(
              // 这是个筛选器不是表单字段：比默认 12/8 更紧一档，别让它在标题
              // 下面横成一条大色块。
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
              minWidth: 220,
              initialValue: _filterId,
              selectedOptionBuilder: (context, value) => Text(
                value.isEmpty ? s.webhookLogFilterAll : _nameOf(value, all),
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(fontSize: 12),
              ),
              options: [
                ShadOption(value: '', child: Text(s.webhookLogFilterAll)),
                for (final e in widget.endpoints)
                  ShadOption(
                    value: e.id,
                    child: Text(e.name.isEmpty ? e.url : e.name),
                  ),
              ],
              onChanged: (v) => setState(() => _filterId = v ?? ''),
            ),
          ),
          const Spacer(),
        ],
      ),
    );
  }

  /// 「投递中」占位行。等价于一条还没有结果的记录，结果到了就地消失。
  Widget _buildPendingRow(S s, AppColors c, int count) {
    final m = AppMetrics.of(context);
    return Container(
      margin: const EdgeInsets.only(bottom: 6),
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: m.brMd,
        border: Border.all(color: m.borderMedium(c.border), width: 1),
      ),
      child: Row(
        children: [
          WebhookSpinner(color: c.textMuted),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              count > 1
                  ? '${s.webhookLogPending} · $count'
                  : s.webhookLogPending,
              style: TextStyle(fontSize: 11.5, color: c.textSecondary),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildRow(S s, AppColors c, WebhookDeliveryEntry d) {
    final m = AppMetrics.of(context);
    final open = _expanded.contains(d.deliveryId);
    final time = DateTime.fromMillisecondsSinceEpoch(d.timestampMs);
    final clock =
        '${_two(time.hour)}:${_two(time.minute)}:${_two(time.second)}';
    final resultColor = d.success ? c.statusSuccess : c.statusError;
    final resultText = d.success
        ? '${d.statusCode} · ${d.latencyMs}ms'
        : (d.statusCode > 0
              ? '${d.statusCode} · ${s.webhookAttempts(d.attempts)}'
              : '${d.error} · ${s.webhookAttempts(d.attempts)}');

    // 详情块**不能**放进折叠用的 GestureDetector 里：在里面点一下就会把行
    // 收起来，用户还没选中就没了。只有头部一行是可点的折叠开关。
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 2, vertical: 9),
      decoration: BoxDecoration(
        border: Border(
          bottom: BorderSide(color: m.borderFade(c.border), width: 1),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          MouseRegion(
            cursor: SystemMouseCursors.click,
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: () => setState(() {
                if (!_expanded.remove(d.deliveryId)) {
                  _expanded.add(d.deliveryId);
                }
              }),
              child: Row(
                children: [
                  Text(
                    clock,
                    style: TextStyle(
                      fontFamily: 'monospace',
                      fontSize: 10.5,
                      color: c.textMuted,
                    ),
                  ),
                  const SizedBox(width: 8),
                  _EventBadge(event: d.event),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      d.endpointName,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 11.5, color: c.textSecondary),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Text(
                    resultText,
                    style: TextStyle(
                      fontFamily: 'monospace',
                      fontSize: 10.5,
                      color: resultColor,
                    ),
                  ),
                ],
              ),
            ),
          ),
          if (open) ...[
            const SizedBox(height: 8),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
              decoration: BoxDecoration(
                color: c.bg,
                borderRadius: m.brMd,
                border: Border.all(color: m.borderMedium(c.border), width: 1),
              ),
              child: SelectableMonoText(text: _detailText(s, d)),
            ),
          ],
        ],
      ),
    );
  }

  String _detailText(S s, WebhookDeliveryEntry d) {
    final buffer = StringBuffer()
      ..writeln('POST ${d.url}')
      ..writeln(d.requestHeaders)
      ..writeln()
      ..writeln(d.requestBody);
    if (d.statusCode > 0) {
      buffer
        ..writeln()
        ..writeln('← ${d.statusCode} ${s.webhookLogResponse}')
        ..writeln(d.responseBody);
    } else if (d.error.isNotEmpty) {
      buffer
        ..writeln()
        ..writeln('← ${d.error}');
    }
    if (d.statusCode >= 400 && d.statusCode < 500) {
      buffer
        ..writeln()
        ..writeln(s.webhookLogHint4xx);
    }
    return buffer.toString().trimRight();
  }

  /// 由 `child` 的 Column 承载，宽度有界。
  Widget _buildFooter(S s) {
    final c = AppColors.of(context);
    final busy = widget.webhook.simulating;
    // 回执说 0 个目标订阅 —— 这时候干等投递记录是等不到的，直接说明白。
    final noTarget = widget.webhook.lastSimulateDispatched == 0;
    return Row(
      children: [
        ShadButton.ghost(
          size: ShadButtonSize.sm,
          onPressed: widget.webhook.clearDeliveries,
          child: Text(s.webhookLogClear),
        ),
        const SizedBox(width: 10),
        // 「模拟一次投递」光看按钮不知道会发生什么，旁边一句话说清楚。
        Expanded(
          child: Text(
            noTarget ? s.webhookSimulateNoTarget : s.webhookLogSimulateHint,
            style: TextStyle(
              fontSize: 10.5,
              height: 1.4,
              color: noTarget ? c.statusWarning : c.textMuted,
            ),
          ),
        ),
        const SizedBox(width: 10),
        ShadButton.outline(
          size: ShadButtonSize.sm,
          enabled: !busy,
          onPressed: widget.webhook.simulate,
          child: busy
              ? Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    WebhookSpinner(color: c.textMuted),
                    const SizedBox(width: 6),
                    Text(s.webhookLogPending),
                  ],
                )
              : Text(s.webhookLogSimulate),
        ),
      ],
    );
  }

  static String _two(int v) => v.toString().padLeft(2, '0');
}

/// 可选中的等宽文本，选区配色跟随主题。
///
/// `SelectableText` 默认从 `DefaultSelectionStyle` 取选区色，`ShadApp` 下
/// 那是 Material 的淡紫，与 13 套配色全都不搭。这里显式接到 accent 上。
class SelectableMonoText extends StatelessWidget {
  const SelectableMonoText({
    super.key,
    required this.text,
    this.fontSize = 10,
    this.height = 1.7,
  });

  final String text;
  final double fontSize;
  final double height;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    return DefaultSelectionStyle(
      selectionColor: m.soft(c.accent),
      cursorColor: c.accent,
      child: SelectableText(
        text,
        style: TextStyle(
          fontFamily: 'monospace',
          fontSize: fontSize,
          height: height,
          color: c.textMuted,
        ),
      ),
    );
  }
}

/// 事件 badge：完成绿 / 失败红 / 其余蓝。
class _EventBadge extends StatelessWidget {
  const _EventBadge({required this.event});

  final String event;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    final color = switch (event) {
      'task.completed' => c.statusSuccess,
      'task.failed' => c.statusError,
      _ => c.accent,
    };
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(
        color: m.subtle(color),
        borderRadius: m.brXs,
        border: Border.all(color: m.soft(color), width: 1),
      ),
      child: Text(
        event,
        style: TextStyle(fontFamily: 'monospace', fontSize: 9.5, color: color),
      ),
    );
  }
}

/// 行内小转圈。
///
/// 投递要走真实网络（超时 10s、失败还重试 3 次）。按钮点下去到结果回来
/// 之间必须有动静，否则用户只会再点一次 —— 而每一次点击都是一条真实的
/// 对外请求。
class WebhookSpinner extends StatefulWidget {
  const WebhookSpinner({super.key, required this.color, this.size = 11});

  final Color color;
  final double size;

  @override
  State<WebhookSpinner> createState() => _WebhookSpinnerState();
}

class _WebhookSpinnerState extends State<WebhookSpinner>
    with SingleTickerProviderStateMixin {
  late final AnimationController _ctrl = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 900),
  )..repeat();

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return RotationTransition(
      turns: _ctrl,
      child: SizedBox(
        width: widget.size,
        height: widget.size,
        child: CustomPaint(painter: _ArcPainter(widget.color)),
      ),
    );
  }
}

class _ArcPainter extends CustomPainter {
  const _ArcPainter(this.color);

  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.6
      ..strokeCap = StrokeCap.round;
    canvas.drawArc(Offset.zero & size, -1.2, 4.4, false, paint);
  }

  @override
  bool shouldRepaint(_ArcPainter old) => old.color != color;
}
