import 'package:flutter/material.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../bindings/bindings.dart';
import '../i18n/locale_provider.dart';
import '../models/webhook_endpoint.dart';
import '../models/webhook_provider.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';
import 'webhook_endpoint_dialog.dart';
import 'webhook_delivery_panel.dart';

/// 行内事件 chips 最多显示几个，多出的折成 `+N`。
///
/// 六个事件全订阅时 chips 会把行撑到 ~590px；设置正文在窄窗口下没这么宽，
/// 于是溢出。截断比让行溢出好——完整订阅集在编辑对话框里看。
const int _kMaxRowChips = 3;

/// 端点列表（一卡多行，行间发丝线，与设置页的分组卡视觉一致）。
class WebhookEndpointList extends StatelessWidget {
  const WebhookEndpointList({
    super.key,
    required this.endpoints,
    required this.webhook,
    required this.pendingDeleteId,
    required this.onToggle,
    required this.onTest,
    required this.onLogs,
    required this.onEdit,
    required this.onDelete,
  });

  final List<WebhookEndpoint> endpoints;
  final WebhookProvider webhook;

  /// 处于「再点一次就删」窗口的端点 id（空 = 无）。
  final String pendingDeleteId;
  final void Function(WebhookEndpoint, bool) onToggle;
  final void Function(WebhookEndpoint) onTest;
  final void Function(WebhookEndpoint) onLogs;
  final void Function(WebhookEndpoint) onEdit;
  final void Function(WebhookEndpoint) onDelete;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    return Container(
      clipBehavior: Clip.antiAlias,
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: m.brDialog,
        border: Border.all(color: m.borderMedium(c.border), width: 1),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (var i = 0; i < endpoints.length; i++) ...[
            if (i > 0)
              Container(
                height: 1,
                margin: const EdgeInsets.only(left: 16),
                color: m.borderFade(c.border),
              ),
            _WebhookEndpointRow(
              endpoint: endpoints[i],
              latest: webhook.latestFor(endpoints[i].id),
              testing: webhook.isTesting(endpoints[i].id),
              armedForDelete: pendingDeleteId == endpoints[i].id,
              onToggle: (v) => onToggle(endpoints[i], v),
              onTest: () => onTest(endpoints[i]),
              onLogs: () => onLogs(endpoints[i]),
              onEdit: () => onEdit(endpoints[i]),
              onDelete: () => onDelete(endpoints[i]),
            ),
          ],
        ],
      ),
    );
  }
}

/// 单个端点行。信息密度从左到右递减：
/// `[字标] 名称 + URL(中段掩码)  [事件 chips]  [健康状态]  [开关]`，
/// hover 时事件 chips 让位给行内操作（测试 / 日志 / 编辑 / 删除）。
class _WebhookEndpointRow extends StatefulWidget {
  const _WebhookEndpointRow({
    required this.endpoint,
    required this.latest,
    required this.testing,
    required this.armedForDelete,
    required this.onToggle,
    required this.onTest,
    required this.onLogs,
    required this.onEdit,
    required this.onDelete,
  });

  final WebhookEndpoint endpoint;

  /// 该端点最近一次投递（null = 尚无记录）。
  final WebhookDeliveryEntry? latest;

  /// 该端点有测试投递在途：按钮转圈 + 拦住连点。
  final bool testing;
  final bool armedForDelete;
  final ValueChanged<bool> onToggle;
  final VoidCallback onTest;
  final VoidCallback onLogs;
  final VoidCallback onEdit;
  final VoidCallback onDelete;

  @override
  State<_WebhookEndpointRow> createState() => _WebhookEndpointRowState();
}

class _WebhookEndpointRowState extends State<_WebhookEndpointRow> {
  bool _hover = false;

  /// URL 中段掩码：保留头尾各一段，中间用 • 顶掉——token 常在路径里。
  static String _maskUrl(String url) {
    if (url.length <= 44) return url;
    return '${url.substring(0, 30)}•••${url.substring(url.length - 8)}';
  }

  @override
  Widget build(BuildContext context) {
    final s = LocaleScope.of(context);
    final c = AppColors.of(context);
    final e = widget.endpoint;
    final enabled = e.enabled;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Container(
        color: _hover ? c.hoverBg : null,
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        child: Opacity(
          opacity: enabled ? 1 : 0.62,
          child: Row(
            children: [
              WebhookPresetTile(presetId: e.preset),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      e.name.isEmpty ? e.url : e.name,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 13, color: c.textPrimary),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      _maskUrl(e.url),
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 10.5,
                        color: c.textMuted,
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 10),
              if (_hover) _buildOps(s, c) else _buildEventChips(s, c),
              const SizedBox(width: 10),
              SizedBox(width: 150, child: _buildHealth(s, c)),
              const SizedBox(width: 10),
              ShadSwitch(value: enabled, onChanged: widget.onToggle),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildEventChips(S s, AppColors c) {
    final events = widget.endpoint.events;
    final shown = events.take(_kMaxRowChips).toList();
    final overflow = events.length - shown.length;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (final event in shown)
          Padding(
            padding: const EdgeInsets.only(left: 4),
            child: _chip(s.webhookEventLabel(event), c),
          ),
        if (overflow > 0)
          Padding(
            padding: const EdgeInsets.only(left: 4),
            child: _chip('+$overflow', c),
          ),
      ],
    );
  }

  Widget _chip(String label, AppColors c) {
    final m = AppMetrics.of(context);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 1),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: m.borderMedium(c.border), width: 1),
      ),
      child: Text(label, style: TextStyle(fontSize: 10, color: c.textMuted)),
    );
  }

  Widget _buildOps(S s, AppColors c) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        _MiniButton(
          label: widget.testing ? s.webhookTesting : s.webhookRowTest,
          onTap: widget.onTest,
          busy: widget.testing,
        ),
        const SizedBox(width: 5),
        _MiniButton(label: s.webhookRowLogs, onTap: widget.onLogs),
        const SizedBox(width: 5),
        _MiniButton(label: s.webhookRowEdit, onTap: widget.onEdit),
        const SizedBox(width: 5),
        _MiniButton(
          label: widget.armedForDelete
              ? s.webhookRowDeleteConfirm
              : s.webhookRowDelete,
          onTap: widget.onDelete,
          danger: widget.armedForDelete,
        ),
      ],
    );
  }

  /// 健康状态 = 投递日志的第一层：不点日志也知道端点死活。
  Widget _buildHealth(S s, AppColors c) {
    final latest = widget.latest;
    final (Color dot, String text) = switch ((
      widget.endpoint.enabled,
      latest,
    )) {
      (false, _) => (c.textMuted, s.webhookHealthDisabled),
      (true, null) => (c.textMuted, s.webhookHealthNone),
      (true, final d?) when d.success => (
        c.statusSuccess,
        s.webhookHealthOk('${d.statusCode} · ${d.latencyMs}ms'),
      ),
      (true, final d?) => (
        c.statusError,
        s.webhookHealthFail(
          d.statusCode > 0
              ? '${d.statusCode} · ${s.webhookAttempts(d.attempts)}'
              : d.error,
        ),
      ),
    };
    return Row(
      mainAxisSize: MainAxisSize.min,
      mainAxisAlignment: MainAxisAlignment.end,
      children: [
        Container(
          width: 6,
          height: 6,
          decoration: BoxDecoration(color: dot, shape: BoxShape.circle),
        ),
        const SizedBox(width: 6),
        Flexible(
          child: Text(
            text,
            overflow: TextOverflow.ellipsis,
            textAlign: TextAlign.right,
            style: TextStyle(
              fontSize: 11,
              color: dot == c.statusError ? c.statusError : c.textMuted,
            ),
          ),
        ),
      ],
    );
  }
}

/// 行内小操作按钮。
///
/// 原来是个没有任何状态反馈的静态方块 —— 鼠标停上去毫无变化，用户不知道
/// 它能不能点。这里给足三态：静息（低对比，不抢戏）、hover（底色抬到
/// accent、边框变实、文字变主色）、busy（转圈 + 拦住重复点击）。
class _MiniButton extends StatefulWidget {
  const _MiniButton({
    required this.label,
    required this.onTap,
    this.danger = false,
    this.busy = false,
  });

  final String label;
  final VoidCallback onTap;
  final bool danger;
  final bool busy;

  @override
  State<_MiniButton> createState() => _MiniButtonState();
}

class _MiniButtonState extends State<_MiniButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    final danger = widget.danger;
    final active = _hover && !widget.busy;

    final Color fg = danger
        ? c.statusError
        : (active ? c.textPrimary : c.textSecondary);
    final Color bg = danger
        ? m.subtle(c.statusError)
        : (active ? c.hoverBg : c.surface2);
    final Color border = danger
        ? c.statusError
        : (active ? m.borderStrong(c.border) : m.borderMedium(c.border));

    return MouseRegion(
      cursor: widget.busy ? SystemMouseCursors.basic : SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        onTap: widget.busy ? null : widget.onTap,
        child: Container(
          // 悬浮态直接切色，不用 AnimatedContainer：与侧栏 `_NavItem` 一致，
          // 也避开从透明基底插值出的中间灰帧。
          padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 3),
          decoration: BoxDecoration(
            color: bg,
            borderRadius: m.brSm,
            border: Border.all(color: border, width: 1),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (widget.busy) ...[
                WebhookSpinner(color: fg, size: 9),
                const SizedBox(width: 5),
              ],
              Text(widget.label, style: TextStyle(fontSize: 11, color: fg)),
            ],
          ),
        ),
      ),
    );
  }
}
