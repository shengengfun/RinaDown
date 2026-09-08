import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shadcn_ui/shadcn_ui.dart';
import '../models/download_controller.dart';
import '../models/download_task.dart';
import '../models/settings_provider.dart';
import '../models/speed_unit.dart';
import '../models/view_prefs.dart';
import '../i18n/locale_provider.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';
import '../services/shutdown_service.dart';
import '../services/system_proxy_status.dart';
import '../services/system_stats_service.dart';
import 'feedback_dialog.dart';

// 预设关机延迟（分钟；0 = 完成后立即关机）
const _kShutdownPresets = [0, 1, 5, 10, 30];

/// 将字节/秒格式化为可读速率字符串，整数不显示小数
String _formatSpeed(int bytes) {
  if (bytes >= 1024 * 1024) {
    final mb = bytes / (1024 * 1024);
    final rounded = mb.round();
    return rounded == mb ? '$rounded MB/s' : '${mb.toStringAsFixed(1)} MB/s';
  }
  return '${(bytes / 1024).round()} KB/s';
}

/// 状态栏性能芯片：灰色小标签 + 数值（tabular 对齐）。
class _PerfChip extends StatelessWidget {
  final String label;
  final String text;
  final AppColors c;
  final Color? color;

  const _PerfChip({
    required this.label,
    required this.text,
    required this.c,
    this.color,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          label,
          style: TextStyle(fontSize: 10, color: c.textMuted),
        ),
        const SizedBox(width: 4),
        Text(
          text,
          style: TextStyle(
            fontSize: 10.5,
            color: color ?? c.textSecondary,
            fontFeatures: const [FontFeature.tabularFigures()],
          ),
        ),
      ],
    );
  }
}

class StatusBar extends StatefulWidget {
  final DownloadController controller;
  final SettingsProvider settingsProvider;
  final ViewPrefsStore viewPrefsStore;

  final VoidCallback? onOpenProxySettings;

  const StatusBar({
    super.key,
    required this.controller,
    required this.settingsProvider,
    required this.viewPrefsStore,
    this.onOpenProxySettings,
  });

  @override
  State<StatusBar> createState() => _StatusBarState();
}

class _StatusBarState extends State<StatusBar> {
  final _popoverController = ShadPopoverController();
  final _customController = TextEditingController();
  final _uploadCustomController = TextEditingController();
  final _shutdownPopoverController = ShadPopoverController();
  final _shutdownMinutesController = TextEditingController();
  final _proxyPopoverController = ShadPopoverController();

  /// 上次已写入 settings 的字节数，用于防循环更新
  int _lastKnownBytes = -1;

  /// 上次已写入 settings 的上传限速字节数，用于防循环更新
  int _lastKnownUploadBytes = -1;

  @override
  void initState() {
    super.initState();
    final bytes = widget.settingsProvider.speedLimitBytes;
    _lastKnownBytes = bytes;
    _customController.text = _valueText(bytes);
    final uploadBytes = widget.settingsProvider.uploadLimitBytes;
    _lastKnownUploadBytes = uploadBytes;
    _uploadCustomController.text = _valueText(uploadBytes);
    _shutdownMinutesController.text =
        ShutdownService.instance.delayMinutes.toString();
    widget.settingsProvider.addListener(_onSettingsChanged);
    _popoverController.addListener(_onPopoverChanged);
    _shutdownPopoverController.addListener(_onShutdownPopoverChanged);
  }

  @override
  void dispose() {
    _popoverController.removeListener(_onPopoverChanged);
    _shutdownPopoverController.removeListener(_onShutdownPopoverChanged);
    widget.settingsProvider.removeListener(_onSettingsChanged);
    _popoverController.dispose();
    _customController.dispose();
    _uploadCustomController.dispose();
    _shutdownPopoverController.dispose();
    _proxyPopoverController.dispose();
    _shutdownMinutesController.dispose();
    super.dispose();
  }

  /// 当前限速单位（设置页「下载限速」上方选择）。
  String get _unit => widget.settingsProvider.speedLimitUnit;

  /// 将 bytes/s 换算为当前单位下的输入框文本（0 → 空字符串）
  String _valueText(int bytes) {
    if (bytes <= 0) return '';
    return valueFromBytes(_unit, bytes).toString();
  }

  /// 设置页（外部）修改限速 / 限速单位时同步输入框
  void _onSettingsChanged() {
    var changed = false;
    final newBytes = widget.settingsProvider.speedLimitBytes;
    if (newBytes != _lastKnownBytes) {
      _lastKnownBytes = newBytes;
      _customController.text = _valueText(newBytes);
      changed = true;
    }
    final newUploadBytes = widget.settingsProvider.uploadLimitBytes;
    if (newUploadBytes != _lastKnownUploadBytes) {
      _lastKnownUploadBytes = newUploadBytes;
      _uploadCustomController.text = _valueText(newUploadBytes);
      changed = true;
    }
    if (changed && mounted) setState(() {});
  }

  /// Popover 关闭时，若已开启限速，则将自定义输入框的当前值写入设置
  void _onPopoverChanged() {
    if (!_popoverController.isOpen) {
      _applyCustomInput();
      _applyUploadCustomInput();
    }
  }

  bool get _isLimited => widget.settingsProvider.speedLimitBytes > 0;

  bool get _isUploadLimited => widget.settingsProvider.uploadLimitBytes > 0;

  /// 开启下载限速：优先用输入框数值；空/非法 → 默认 512（当前单位）。
  void _toggleLimit(bool on) {
    if (on) {
      final v = int.tryParse(_customController.text.trim()) ?? 0;
      final effective = v > 0 ? v : 512;
      if (v <= 0) _customController.text = '512';
      final bytes = bytesFromValue(_unit, effective);
      _lastKnownBytes = bytes;
      widget.settingsProvider.setSpeedLimitBytes(bytes);
    } else {
      _lastKnownBytes = 0;
      widget.settingsProvider.setSpeedLimitBytes(0);
    }
  }

  /// 自定义输入框的值写入设置（仅限速已开启时有效）
  void _applyCustomInput() {
    if (!_isLimited) return;
    final v = int.tryParse(_customController.text.trim()) ?? 0;
    if (v > 0) {
      final bytes = bytesFromValue(_unit, v);
      if (bytes != _lastKnownBytes) {
        _lastKnownBytes = bytes;
        widget.settingsProvider.setSpeedLimitBytes(bytes);
      }
    }
  }

  /// 切换上传限速开关（全局 BT 上传，与设置页同源）
  void _toggleUploadLimit(bool on) {
    if (on) {
      final v = int.tryParse(_uploadCustomController.text.trim()) ?? 0;
      final effective = v > 0 ? v : 512;
      if (v <= 0) _uploadCustomController.text = '512';
      final bytes = bytesFromValue(_unit, effective);
      _lastKnownUploadBytes = bytes;
      widget.settingsProvider.setUploadLimitBytes(bytes);
    } else {
      _lastKnownUploadBytes = 0;
      widget.settingsProvider.setUploadLimitBytes(0);
    }
  }

  /// 上传自定义输入框的值写入设置（仅上传限速已开启时有效）
  void _applyUploadCustomInput() {
    if (!_isUploadLimited) return;
    final v = int.tryParse(_uploadCustomController.text.trim()) ?? 0;
    if (v > 0) {
      final bytes = bytesFromValue(_unit, v);
      if (bytes != _lastKnownUploadBytes) {
        _lastKnownUploadBytes = bytes;
        widget.settingsProvider.setUploadLimitBytes(bytes);
      }
    }
  }

  // ---------------------------------------------------------------------------
  // 完成后关机
  // ---------------------------------------------------------------------------

  /// Popover 关闭时，若已开启关机，则应用自定义分钟输入
  void _onShutdownPopoverChanged() {
    if (!_shutdownPopoverController.isOpen) {
      _applyShutdownMinutesInput();
    }
  }

  /// 切换「完成后关机」开关
  void _toggleShutdown(bool on) {
    final svc = ShutdownService.instance;
    if (on) {
      // 空/非法输入 → 保持服务当前延迟；"0" = 立即关机
      final minutes = int.tryParse(_shutdownMinutesController.text.trim());
      final armed = svc.arm(minutes: minutes);
      if (armed) {
        _shutdownMinutesController.text = svc.delayMinutes.toString();
      }
    } else {
      svc.cancel();
    }
  }

  /// 点击预设分钟：设置延迟并（可开启时）直接开启
  void _applyShutdownPreset(int minutes) {
    final svc = ShutdownService.instance;
    _shutdownMinutesController.text = minutes.toString();
    if (svc.isArmed) {
      svc.setDelayMinutes(minutes);
    } else {
      svc.arm(minutes: minutes);
    }
  }

  /// 自定义分钟输入写入服务（仅已开启时有效；0 = 立即关机）
  void _applyShutdownMinutesInput() {
    final svc = ShutdownService.instance;
    final minutes = int.tryParse(_shutdownMinutesController.text.trim());
    if (minutes != null && svc.isArmed) {
      svc.setDelayMinutes(minutes);
      _shutdownMinutesController.text = svc.delayMinutes.toString();
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final s = LocaleScope.of(context);
    return ListenableBuilder(
      listenable: Listenable.merge([
        widget.controller,
        widget.settingsProvider,
        widget.viewPrefsStore,
        ShutdownService.instance,
      ]),
      builder: (context, _) {
        final dlSpeed = DownloadTask.formatBytes(
          widget.controller.totalDownloadSpeed,
        );
        final upSpeedBps = widget.controller.totalUploadSpeed;
        final ulSpeed = DownloadTask.formatBytes(upSpeedBps);
        final active = widget.controller.activeCount;
        final paused = widget.controller.pausedCount;
        final total = widget.controller.tasks.length;

        final tab = widget.controller.statusTab.name;
        final prefs = widget.viewPrefsStore.resolve(tab);
        final visibleCount = widget.controller.visibleEntityExpandedCount(
          prefs,
        );
        final hiddenCompleted = widget.controller.hiddenCompletedCount(prefs);
        final scopeSizeText = DownloadTask.formatBytes(
          widget.controller
              .buildListSections(prefs)
              .expand((section) => section.entities)
              .fold<int>(0, (sum, e) => sum + e.totalBytes),
        );

        return Container(
          height: 28,
          padding: const EdgeInsets.symmetric(horizontal: 16),
          decoration: BoxDecoration(
            color: c.surface1,
            border: Border(top: BorderSide(color: c.border, width: 1)),
          ),
          child: Row(
            children: [
              // 状态指示
              Row(
                children: [
                  Icon(
                    LucideIcons.circle,
                    size: 8,
                    color: active > 0 ? AppColors.green : c.textMuted,
                  ),
                  const SizedBox(width: 6),
                  Text(
                    active > 0 ? s.statusDownloadingLabel : s.statusIdle,
                    style: TextStyle(fontSize: 10.5, color: c.textMuted),
                  ),
                ],
              ),
              const SizedBox(width: 20),
              // 实时下载速度
              Row(
                children: [
                  const Icon(
                    LucideIcons.arrowDown,
                    size: 10,
                    color: AppColors.green,
                  ),
                  const SizedBox(width: 4),
                  Text(
                    '$dlSpeed/s',
                    style: TextStyle(
                      fontSize: 10.5,
                      color: c.textMuted,
                      fontFeatures: const [FontFeature.tabularFigures()],
                    ),
                  ),
                ],
              ),
              // 实时上传速度（有做种任务或 BT 上行流量时显示）
              if (widget.controller.seedingCount > 0 || upSpeedBps > 0) ...[
                const SizedBox(width: 12),
                Row(
                  children: [
                    const Icon(
                      LucideIcons.arrowUp,
                      size: 10,
                      color: AppColors.green,
                    ),
                    const SizedBox(width: 4),
                    Text(
                      '$ulSpeed/s',
                      style: TextStyle(
                        fontSize: 10.5,
                        color: c.textMuted,
                        fontFeatures: const [FontFeature.tabularFigures()],
                      ),
                    ),
                  ],
                ),
              ],
              const SizedBox(width: 20),
              Text(
                s.statusSummary(active, paused, total),
                style: TextStyle(fontSize: 10.5, color: c.textMuted),
              ),
              const SizedBox(width: 20),
              // 视图作用域摘要（design-proto-spec §11 `renderStatusbar` 左段）：
              // N 个任务 · 合计大小 [· 已隐藏 M 个已完成]
              //
              // Expanded（非 Flexible）：吃满全部剩余空间把右簇顶到行尾。
              // 此前 Flexible(loose) 与 Spacer 各分一半剩余空间，Flexible
              // 没用掉的配额不会回流给 Spacer，按 mainAxisAlignment.start
              // 沉积在行尾——表现为「反馈」右侧一大段空白且随窗口变宽。
              Expanded(
                child: Text(
                  hiddenCompleted > 0
                      ? '${s.statusScopeSummary(visibleCount, scopeSizeText)}'
                            '${s.statusScopeHidden(hiddenCompleted)}'
                      : s.statusScopeSummary(visibleCount, scopeSizeText),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 10.5,
                    color: c.textMuted,
                    fontFeatures: const [FontFeature.tabularFigures()],
                  ),
                ),
              ),
              // 性能监控（CPU / 内存 / 上下行）— 位于「限速」左侧
              ListenableBuilder(
                listenable: SystemStatsService.instance,
                builder: (context, _) {
                  final st = SystemStatsService.instance;
                  if (!st.active) return const SizedBox.shrink();
                  return Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      _PerfChip(
                        label: s.perfCpu,
                        text: '${st.cpuPercent.round()}%',
                        c: c,
                      ),
                      const SizedBox(width: 10),
                      _PerfChip(
                        label: s.perfRam,
                        text: '${st.ramPercent.round()}%',
                        c: c,
                      ),
                      if (st.netDownBps > 0 || st.netUpBps > 0) ...[
                        const SizedBox(width: 10),
                        _PerfChip(
                          label: s.perfNetD,
                          text: _formatSpeed(st.netDownBps),
                          c: c,
                          color: AppColors.green,
                        ),
                        const SizedBox(width: 10),
                        _PerfChip(
                          label: s.perfNetU,
                          text: _formatSpeed(st.netUpBps),
                          c: c,
                          color: AppColors.green,
                        ),
                      ],
                    ],
                  );
                },
              ),
              const SizedBox(width: 16),
              // 限速 Popover 触发器
              _SpeedLimitTrigger(
                popoverController: _popoverController,
                settingsProvider: widget.settingsProvider,
                customController: _customController,
                uploadCustomController: _uploadCustomController,
                isLimited: _isLimited,
                limitBytes: widget.settingsProvider.speedLimitBytes,
                isUploadLimited: _isUploadLimited,
                uploadLimitBytes: widget.settingsProvider.uploadLimitBytes,
                unit: _unit,
                onToggle: _toggleLimit,
                onApplyCustom: _applyCustomInput,
                onToggleUpload: _toggleUploadLimit,
                onApplyUploadCustom: _applyUploadCustomInput,
                s: s,
                c: c,
              ),
              const SizedBox(width: 12),
              Container(width: 1, height: 12, color: c.border),
              const SizedBox(width: 12),
              // 完成后关机 Popover 触发器
              _ShutdownTrigger(
                popoverController: _shutdownPopoverController,
                controller: widget.controller,
                minutesController: _shutdownMinutesController,
                onToggle: _toggleShutdown,
                onApplyPreset: _applyShutdownPreset,
                onApplyCustom: _applyShutdownMinutesInput,
                s: s,
                c: c,
              ),
              const SizedBox(width: 12),
              Container(width: 1, height: 12, color: c.border),
              const SizedBox(width: 12),
              // 代理模式 Popover 触发器
              _ProxyModeTrigger(
                popoverController: _proxyPopoverController,
                settingsProvider: widget.settingsProvider,
                onOpenProxySettings: widget.onOpenProxySettings,
                s: s,
                c: c,
              ),
              const SizedBox(width: 12),
              Container(width: 1, height: 12, color: c.border),
              const SizedBox(width: 12),
              // 反馈按钮
              GestureDetector(
                onTap: () => showFeedbackDialog(context),
                child: MouseRegion(
                  cursor: SystemMouseCursors.click,
                  child: Row(
                    children: [
                      Icon(
                        LucideIcons.messageSquarePlus,
                        size: 11,
                        color: c.textMuted,
                      ),
                      const SizedBox(width: 4),
                      Text(
                        s.feedback,
                        style: TextStyle(fontSize: 10.5, color: c.textMuted),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

// =============================================================================
// 触发器 Widget — 显示当前限速状态，点击展开/收起 Popover
// =============================================================================

class _SpeedLimitTrigger extends StatelessWidget {
  final ShadPopoverController popoverController;
  final SettingsProvider settingsProvider;
  final TextEditingController customController;
  final TextEditingController uploadCustomController;
  final bool isLimited;
  final int limitBytes;
  final bool isUploadLimited;
  final int uploadLimitBytes;
  final String unit;
  final ValueChanged<bool> onToggle;
  final VoidCallback onApplyCustom;
  final ValueChanged<bool> onToggleUpload;
  final VoidCallback onApplyUploadCustom;
  final S s;
  final AppColors c;

  const _SpeedLimitTrigger({
    required this.popoverController,
    required this.settingsProvider,
    required this.customController,
    required this.uploadCustomController,
    required this.isLimited,
    required this.limitBytes,
    required this.isUploadLimited,
    required this.uploadLimitBytes,
    required this.unit,
    required this.onToggle,
    required this.onApplyCustom,
    required this.onToggleUpload,
    required this.onApplyUploadCustom,
    required this.s,
    required this.c,
  });

  @override
  Widget build(BuildContext context) {
    final triggerColor = isLimited || isUploadLimited ? c.accent : c.textMuted;
    // 收起态文案：仅下载限速时维持原样；有上传限速时以 ↓/↑ 前缀区分两向。
    final String triggerText;
    if (isLimited && isUploadLimited) {
      triggerText =
          '↓${_formatSpeed(limitBytes)} · ↑${_formatSpeed(uploadLimitBytes)}';
    } else if (isUploadLimited) {
      triggerText = '↑${_formatSpeed(uploadLimitBytes)}';
    } else if (isLimited) {
      triggerText = _formatSpeed(limitBytes);
    } else {
      triggerText = s.statusSpeedLimitOff;
    }

    return ShadPopover(
      controller: popoverController,
      effects: const [],
      // 弹出在触发器上方，右对齐（状态栏位于屏幕底部）。
      // 用手动 ShadAnchor 精确锚定：ShadAnchorAuto 底层按目标点水平居中，
      // 右对齐配置会整体向右偏移一个弹层宽度。
      anchor: const ShadAnchor(
        childAlignment: Alignment.bottomRight,
        overlayAlignment: Alignment.topRight,
        offset: Offset(0, -8),
      ),
      padding: EdgeInsets.zero,
      // 使用 ListenableBuilder 确保 Popover 内容在设置变更后自动刷新
      popover: (ctx) => ListenableBuilder(
        listenable: settingsProvider,
        builder: (ctx2, _) => _SpeedLimitPopoverContent(
          customController: customController,
          uploadCustomController: uploadCustomController,
          isLimited: settingsProvider.speedLimitBytes > 0,
          limitBytes: settingsProvider.speedLimitBytes,
          isUploadLimited: settingsProvider.uploadLimitBytes > 0,
          uploadLimitBytes: settingsProvider.uploadLimitBytes,
          unit: settingsProvider.speedLimitUnit,
          onToggle: onToggle,
          onApplyCustom: onApplyCustom,
          onToggleUpload: onToggleUpload,
          onApplyUploadCustom: onApplyUploadCustom,
          s: s,
          c: c,
        ),
      ),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        child: GestureDetector(
          onTap: popoverController.toggle,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(LucideIcons.gauge, size: 11, color: triggerColor),
              const SizedBox(width: 4),
              Text(
                triggerText,
                style: TextStyle(
                  fontSize: 10.5,
                  color: triggerColor,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
              const SizedBox(width: 2),
              Icon(LucideIcons.chevronUp, size: 9, color: triggerColor),
            ],
          ),
        ),
      ),
    );
  }
}

// =============================================================================
// Popover 内容 — 开关 + 预设速率 + 自定义输入
// =============================================================================

class _SpeedLimitPopoverContent extends StatelessWidget {
  final TextEditingController customController;
  final TextEditingController uploadCustomController;
  final bool isLimited;
  final int limitBytes;
  final bool isUploadLimited;
  final int uploadLimitBytes;
  final String unit;
  final ValueChanged<bool> onToggle;
  final VoidCallback onApplyCustom;
  final ValueChanged<bool> onToggleUpload;
  final VoidCallback onApplyUploadCustom;
  final S s;
  final AppColors c;

  const _SpeedLimitPopoverContent({
    required this.customController,
    required this.uploadCustomController,
    required this.isLimited,
    required this.limitBytes,
    required this.isUploadLimited,
    required this.uploadLimitBytes,
    required this.unit,
    required this.onToggle,
    required this.onApplyCustom,
    required this.onToggleUpload,
    required this.onApplyUploadCustom,
    required this.s,
    required this.c,
  });

  @override
  Widget build(BuildContext context) {
    final m = AppMetrics.of(context);
    return SizedBox(
      width: 220,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          // ── 下载限速 ──
          _limitSection(
            m: m,
            title: s.speedLimitTitle,
            sectionLimited: isLimited,
            sectionBytes: limitBytes,
            unit: unit,
            controller: customController,
            onSectionToggle: onToggle,
            onSectionCustom: onApplyCustom,
          ),
          Divider(color: c.border, height: 1),
          // ── 上传限速（全局 BT 上传，与设置页同源）──
          _limitSection(
            m: m,
            title: s.uploadLimit,
            sectionLimited: isUploadLimited,
            sectionBytes: uploadLimitBytes,
            unit: unit,
            controller: uploadCustomController,
            onSectionToggle: onToggleUpload,
            onSectionCustom: onApplyUploadCustom,
          ),
        ],
      ),
    );
  }

  /// 单个限速区块：标题行 + 开关、分割线、自定义输入。
  /// 下载/上传两区结构完全一致，仅数据源与回调不同。
  Widget _limitSection({
    required AppMetrics m,
    required String title,
    required bool sectionLimited,
    required int sectionBytes,
    required String unit,
    required TextEditingController controller,
    required ValueChanged<bool> onSectionToggle,
    required VoidCallback onSectionCustom,
  }) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        // 标题行 + 开关
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 12, 8, 10),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  title,
                  style: TextStyle(
                    fontSize: 12.5,
                    fontWeight: FontWeight.w600,
                    color: c.textPrimary,
                  ),
                ),
              ),
              ShadSwitch(
                value: sectionLimited,
                onChanged: onSectionToggle,
                width: 34,
                height: 18,
                margin: 2,
              ),
            ],
          ),
        ),
        // 分割线
        Divider(color: c.border, height: 1),
        // 自定义输入（仅保留文本框；预设便捷按钮已移除）
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 10, 12, 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                s.speedLimitCustom,
                style: TextStyle(fontSize: 11, color: c.textMuted),
              ),
              const SizedBox(height: 6),
              Row(
                children: [
                  Expanded(
                    child: ShadInput(
                      controller: controller,
                      keyboardType: TextInputType.number,
                      inputFormatters: [
                        FilteringTextInputFormatter.digitsOnly,
                      ],
                      placeholder: Text(s.statusSpeedLimitHint),
                      onSubmitted: (_) => onSectionCustom(),
                    ),
                  ),
                  const SizedBox(width: 6),
                  Text(
                    unit,
                    style: TextStyle(fontSize: 12, color: c.textMuted),
                  ),
                ],
              ),
            ],
          ),
        ),
      ],
    );
  }
}

// =============================================================================
// 完成后关机 — 触发器 Widget
// =============================================================================

class _ShutdownTrigger extends StatelessWidget {
  final ShadPopoverController popoverController;
  final DownloadController controller;
  final TextEditingController minutesController;
  final ValueChanged<bool> onToggle;
  final ValueChanged<int> onApplyPreset;
  final VoidCallback onApplyCustom;
  final S s;
  final AppColors c;

  const _ShutdownTrigger({
    required this.popoverController,
    required this.controller,
    required this.minutesController,
    required this.onToggle,
    required this.onApplyPreset,
    required this.onApplyCustom,
    required this.s,
    required this.c,
  });

  @override
  Widget build(BuildContext context) {
    final svc = ShutdownService.instance;
    final Color triggerColor;
    final String triggerText;
    if (svc.isCountingDown) {
      triggerColor = c.statusWarning;
      triggerText = s.shutdownCountdown(svc.remainingText);
    } else if (svc.isArmed) {
      triggerColor = c.accent;
      triggerText = s.shutdownTriggerLabel;
    } else {
      triggerColor = c.textMuted;
      triggerText = s.shutdownTriggerLabel;
    }

    return ShadPopover(
      controller: popoverController,
      effects: const [],
      // 弹出在触发器上方，右对齐（同限速 Popover，手动锚避免 Auto 右偏）
      anchor: const ShadAnchor(
        childAlignment: Alignment.bottomRight,
        overlayAlignment: Alignment.topRight,
        offset: Offset(0, -8),
      ),
      padding: EdgeInsets.zero,
      // 监听服务与控制器 —— 倒计时秒数刷新、活跃任务数变化时开关可用性刷新
      popover: (ctx) => ListenableBuilder(
        listenable: Listenable.merge([svc, controller]),
        builder: (ctx2, _) => _ShutdownPopoverContent(
          minutesController: minutesController,
          onToggle: onToggle,
          onApplyPreset: onApplyPreset,
          onApplyCustom: onApplyCustom,
          s: s,
          c: c,
        ),
      ),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        child: GestureDetector(
          onTap: popoverController.toggle,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(LucideIcons.power, size: 11, color: triggerColor),
              const SizedBox(width: 4),
              Text(
                triggerText,
                style: TextStyle(
                  fontSize: 10.5,
                  color: triggerColor,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
              const SizedBox(width: 2),
              Icon(LucideIcons.chevronUp, size: 9, color: triggerColor),
            ],
          ),
        ),
      ),
    );
  }
}

// =============================================================================
// 完成后关机 — Popover 内容：开关 + 预设延迟 + 自定义分钟 + 倒计时/取消
// =============================================================================

class _ShutdownPopoverContent extends StatelessWidget {
  final TextEditingController minutesController;
  final ValueChanged<bool> onToggle;
  final ValueChanged<int> onApplyPreset;
  final VoidCallback onApplyCustom;
  final S s;
  final AppColors c;

  const _ShutdownPopoverContent({
    required this.minutesController,
    required this.onToggle,
    required this.onApplyPreset,
    required this.onApplyCustom,
    required this.s,
    required this.c,
  });

  @override
  Widget build(BuildContext context) {
    final svc = ShutdownService.instance;
    final canInteract = svc.canArm || svc.isArmed;
    final m = AppMetrics.of(context);

    return SizedBox(
      width: 240,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          // 标题行 + 开关
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 12, 8, 10),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    s.shutdownTitle,
                    style: TextStyle(
                      fontSize: 12.5,
                      fontWeight: FontWeight.w600,
                      color: c.textPrimary,
                    ),
                  ),
                ),
                ShadSwitch(
                  value: svc.isArmed,
                  onChanged: canInteract ? onToggle : null,
                  width: 34,
                  height: 18,
                  margin: 2,
                ),
              ],
            ),
          ),
          // 无活跃任务提示
          if (!canInteract)
            Padding(
              padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
              child: Text(
                s.shutdownNeedActiveTask,
                style: TextStyle(fontSize: 11, color: c.textMuted),
              ),
            ),
          // 倒计时状态 + 取消按钮
          if (svc.isCountingDown) ...[
            Padding(
              padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
              child: Row(
                children: [
                  Icon(LucideIcons.timer, size: 12, color: c.statusWarning),
                  const SizedBox(width: 5),
                  Expanded(
                    child: Text(
                      s.shutdownCountdown(svc.remainingText),
                      style: TextStyle(
                        fontSize: 11.5,
                        fontWeight: FontWeight.w600,
                        color: c.statusWarning,
                        fontFeatures: const [FontFeature.tabularFigures()],
                      ),
                    ),
                  ),
                  ShadButton.destructive(
                    height: 24,
                    padding: const EdgeInsets.symmetric(horizontal: 10),
                    onPressed: svc.cancel,
                    child: Text(
                      s.shutdownCancelButton,
                      style: const TextStyle(fontSize: 11),
                    ),
                  ),
                ],
              ),
            ),
          ] else if (svc.isArmed)
            Padding(
              padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
              child: Text(
                svc.delayMinutes == 0
                    ? s.shutdownArmedHintImmediate
                    : s.shutdownArmedHint(svc.delayMinutes),
                style: TextStyle(fontSize: 11, color: c.textMuted),
              ),
            ),
          // 预设延迟 chips
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
            child: Wrap(
              spacing: 5,
              runSpacing: 5,
              children: _kShutdownPresets.map((minutes) {
                final isSelected =
                    svc.isArmed && svc.delayMinutes == minutes;
                return MouseRegion(
                  cursor: canInteract
                      ? SystemMouseCursors.click
                      : SystemMouseCursors.basic,
                  child: GestureDetector(
                    onTap: canInteract ? () => onApplyPreset(minutes) : null,
                    child: AnimatedContainer(
                      duration: const Duration(milliseconds: 120),
                      padding: const EdgeInsets.symmetric(
                        horizontal: 8,
                        vertical: 4,
                      ),
                      decoration: BoxDecoration(
                        color: isSelected ? c.accent : c.surface2,
                        borderRadius: m.brSm,
                        border: Border.all(
                          color: isSelected ? c.accent : c.border,
                          width: 0.5,
                        ),
                      ),
                      child: Text(
                        minutes == 0
                            ? s.shutdownImmediate
                            : s.shutdownDelayMinutes(minutes),
                        style: TextStyle(
                          fontSize: 11,
                          color: isSelected
                              ? const Color(0xFFFFFFFF)
                              : canInteract
                                  ? c.textSecondary
                                  : c.textDisabled,
                          fontFeatures: const [FontFeature.tabularFigures()],
                        ),
                      ),
                    ),
                  ),
                );
              }).toList(),
            ),
          ),
          // 分割线
          Divider(color: c.border, height: 1),
          // 自定义分钟输入
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 10, 12, 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  s.shutdownDelayLabel,
                  style: TextStyle(fontSize: 11, color: c.textMuted),
                ),
                const SizedBox(height: 6),
                Row(
                  children: [
                    Expanded(
                      child: ShadInput(
                        controller: minutesController,
                        enabled: canInteract,
                        keyboardType: TextInputType.number,
                        inputFormatters: [
                          FilteringTextInputFormatter.digitsOnly,
                        ],
                        onSubmitted: (_) => onApplyCustom(),
                      ),
                    ),
                    const SizedBox(width: 6),
                    Text(
                      s.shutdownMinutesUnit,
                      style: TextStyle(fontSize: 12, color: c.textMuted),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

// =============================================================================
// 代理模式快切 — 触发器 Widget：图标 + 当前模式短标签，点击展开/收起 Popover
// =============================================================================

class _ProxyModeTrigger extends StatelessWidget {
  final ShadPopoverController popoverController;
  final SettingsProvider settingsProvider;
  final VoidCallback? onOpenProxySettings;
  final S s;
  final AppColors c;

  const _ProxyModeTrigger({
    required this.popoverController,
    required this.settingsProvider,
    required this.onOpenProxySettings,
    required this.s,
    required this.c,
  });

  String _modeLabel(String mode) => switch (mode) {
    'system' => s.proxyModeSystem,
    'manual' => s.proxyModeManual,
    'auto' => s.proxyModeAuto,
    _ => s.proxyModeNone,
  };

  @override
  Widget build(BuildContext context) {
    return ShadPopover(
      controller: popoverController,
      // RinaDown 弹出层无进出场动画(rule: shad-overlay-no-animation)。
      effects: const [],
      // 弹出在触发器上方，右对齐（同限速 Popover，手动锚避免 Auto 右偏）
      anchor: const ShadAnchor(
        childAlignment: Alignment.bottomRight,
        overlayAlignment: Alignment.topRight,
        offset: Offset(0, -8),
      ),
      padding: EdgeInsets.zero,
      // 监听设置与系统代理检测状态 —— 模式切换、检测结果到达时刷新
      popover: (ctx) => ListenableBuilder(
        listenable: Listenable.merge([
          settingsProvider,
          SystemProxyStatusService.instance,
        ]),
        builder: (ctx2, _) => _ProxyModeContent(
          settingsProvider: settingsProvider,
          popoverController: popoverController,
          onOpenProxySettings: onOpenProxySettings,
          s: s,
          c: c,
        ),
      ),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        child: GestureDetector(
          onTap: () {
            // 打开弹层时触发系统代理检测（在途去重由服务保证）
            if (!popoverController.isOpen) {
              SystemProxyStatusService.instance.refresh();
            }
            popoverController.toggle();
          },
          // 模式变化仅经 settingsProvider 通知，触发器需自行监听刷新标签
          child: ListenableBuilder(
            listenable: settingsProvider,
            builder: (ctx2, _) {
              final mode = settingsProvider.proxyMode;
              final active = mode != 'none';
              final triggerColor = active ? c.accent : c.textMuted;
              return Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(LucideIcons.globe, size: 11, color: triggerColor),
                  const SizedBox(width: 4),
                  Text(
                    _modeLabel(mode),
                    style: TextStyle(fontSize: 10.5, color: triggerColor),
                  ),
                  const SizedBox(width: 2),
                  Icon(LucideIcons.chevronUp, size: 9, color: triggerColor),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}

// =============================================================================
// 代理模式快切 — Popover 内容：四个模式选项 + 跳转设置入口
// =============================================================================

class _ProxyModeContent extends StatelessWidget {
  final SettingsProvider settingsProvider;
  final ShadPopoverController popoverController;
  final VoidCallback? onOpenProxySettings;
  final S s;
  final AppColors c;

  const _ProxyModeContent({
    required this.settingsProvider,
    required this.popoverController,
    required this.onOpenProxySettings,
    required this.s,
    required this.c,
  });

  void _select(String mode) {
    settingsProvider.setProxyMode(mode);
    popoverController.hide();
  }

  @override
  Widget build(BuildContext context) {
    final svc = SystemProxyStatusService.instance;
    final manualUrl = manualProxyUrlFromSettings(settingsProvider);
    final manualAvailable = manualUrl != null;
    final systemAvailable = svc.detected;
    final current = settingsProvider.proxyMode;

    // 系统代理副文本：检测中 / 已检测到的摘要 / 未检测到
    final String systemSubtitle;
    if (svc.detecting) {
      systemSubtitle = s.proxySystemDetecting;
    } else if (systemAvailable) {
      systemSubtitle = svc.summary;
    } else {
      systemSubtitle = s.proxySystemNotDetected;
    }

    return SizedBox(
      width: 240,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 12, 12, 6),
            child: Text(
              s.statusBarProxyLabel,
              style: TextStyle(
                fontSize: 12.5,
                fontWeight: FontWeight.w600,
                color: c.textPrimary,
              ),
            ),
          ),
          _ProxyModeOption(
            label: s.proxyModeNone,
            selected: current == 'none',
            enabled: true,
            onTap: () => _select('none'),
            c: c,
          ),
          _ProxyModeOption(
            label: s.proxyModeSystem,
            subtitle: systemSubtitle,
            selected: current == 'system',
            enabled: systemAvailable,
            onTap: () => _select('system'),
            c: c,
          ),
          _ProxyModeOption(
            label: s.proxyModeManual,
            subtitle: manualUrl ?? s.proxyNotConfigured,
            selected: current == 'manual',
            enabled: manualAvailable,
            onTap: () => _select('manual'),
            c: c,
          ),
          _ProxyModeOption(
            label: s.proxyModeAuto,
            subtitle: (systemAvailable || manualAvailable)
                ? null
                : s.proxyNotConfigured,
            selected: current == 'auto',
            enabled: systemAvailable || manualAvailable,
            onTap: () => _select('auto'),
            c: c,
          ),
          if (onOpenProxySettings != null) ...[
            const SizedBox(height: 4),
            Divider(color: c.border, height: 1),
            const SizedBox(height: 4),
            _ProxyModeOption(
              label: s.proxyConfigureInSettings,
              selected: false,
              enabled: true,
              muted: true,
              onTap: () {
                popoverController.hide();
                onOpenProxySettings!();
              },
              c: c,
            ),
          ],
          const SizedBox(height: 4),
        ],
      ),
    );
  }
}

// 单个模式选项行：文字左缘与标题对齐；选中以「底色 + 行尾对勾」表达
// （字重字色不跳变，与侧栏选中行同语言）；hover 给 surface2 反馈；
// 禁用整体单级降透明。副文本恒 textMuted 小一号。
class _ProxyModeOption extends StatefulWidget {
  final String label;
  final String? subtitle;
  final bool selected;
  final bool enabled;
  final bool muted;
  final VoidCallback onTap;
  final AppColors c;

  const _ProxyModeOption({
    required this.label,
    this.subtitle,
    required this.selected,
    required this.enabled,
    this.muted = false,
    required this.onTap,
    required this.c,
  });

  @override
  State<_ProxyModeOption> createState() => _ProxyModeOptionState();
}

class _ProxyModeOptionState extends State<_ProxyModeOption> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final c = widget.c;
    final m = AppMetrics.of(context);
    final subtitle = widget.subtitle;

    // 悬浮/选中是即时状态切换：普通 Container 直接切色，不加动画——
    // AnimatedContainer 从透明黑(0x00000000)插值到浅色会经过半透明灰
    // 中间帧，表现为悬浮时深浅两色闪烁(rule: no-lerp-from-transparent)。
    final row = Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
      decoration: BoxDecoration(
        color: widget.selected
            ? c.selectedBg
            : (_hovered && widget.enabled)
            ? c.surface2
            : null,
        borderRadius: m.brSm,
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  widget.label,
                  style: TextStyle(
                    fontSize: 12,
                    height: 1.3,
                    color: widget.muted ? c.textSecondary : c.textPrimary,
                  ),
                ),
                if (subtitle != null && subtitle.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(top: 2),
                    child: Text(
                      subtitle,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 10.5,
                        height: 1.3,
                        color: c.textMuted,
                        fontFeatures: const [FontFeature.tabularFigures()],
                      ),
                    ),
                  ),
              ],
            ),
          ),
          if (widget.selected) ...[
            const SizedBox(width: 8),
            Icon(LucideIcons.check, size: 12, color: c.accent),
          ],
        ],
      ),
    );

    return Padding(
      // 外层 4 + 内层 8 = 12，文字左缘与弹层标题对齐。
      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
      child: MouseRegion(
        cursor: widget.enabled
            ? SystemMouseCursors.click
            : SystemMouseCursors.basic,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          onTap: widget.enabled ? widget.onTap : null,
          behavior: HitTestBehavior.opaque,
          child: Opacity(opacity: widget.enabled ? 1.0 : 0.45, child: row),
        ),
      ),
    );
  }
}
