// 任务代理选择器 —— 新建下载对话框与快速下载表单（主窗 / 独立小窗双宿主）
// 共用。纯展示组件，不依赖 bindings/rinf，popup 独立窗进程可安全 import；
// 禁用态数据（系统代理检测结果 / 全局手动代理配置）由宿主注入。
//
// wire 语义见 models/task_proxy_choice.dart。

import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../i18n/locale_provider.dart';
import '../models/task_proxy_choice.dart';
import '../theme/app_colors.dart';

/// 五选项下拉（跟随全局 / 直连 / 系统代理 / 全局手动 / 自定义）+
/// 仅自定义时显示的 URL 输入框。
///
/// 禁用规则（与 footer 快速切换一致）：
/// - 「系统代理」仅 [systemProxyDetected] 时可选；
/// - 「手动代理（全局配置）」仅 [manualProxyUrl] 非空时可选；
/// 禁用项仍显示但灰置，副文本说明原因（未检测到 / 未配置）。
class TaskProxySelector extends StatelessWidget {
  /// 当前选择项。
  final TaskProxyChoice value;

  /// 选择变更回调（仅可选项会触发）。
  final ValueChanged<TaskProxyChoice> onChanged;

  /// 是否检测到系统代理（数据源：主窗 = SystemProxyStatusService，
  /// popup = QuickPopupPayload.systemProxyDetected）。
  final bool systemProxyDetected;

  /// 检测到的系统代理摘要，如 'http://127.0.0.1:7890'；未检测到为 ''。
  final String systemProxySummary;

  /// 全局手动代理 URL（'' = 未配置）。
  final String manualProxyUrl;

  /// 自定义代理 URL 输入控制器（仅 [TaskProxyChoice.custom] 时展示输入框）。
  final TextEditingController customController;

  const TaskProxySelector({
    super.key,
    required this.value,
    required this.onChanged,
    required this.systemProxyDetected,
    required this.systemProxySummary,
    required this.manualProxyUrl,
    required this.customController,
  });

  String _label(S s, TaskProxyChoice c) => switch (c) {
    TaskProxyChoice.followGlobal => s.taskProxyChoiceFollow,
    TaskProxyChoice.direct => s.taskProxyChoiceDirect,
    TaskProxyChoice.system => s.taskProxyChoiceSystem,
    TaskProxyChoice.globalManual => s.taskProxyChoiceGlobalManual,
    TaskProxyChoice.custom => s.taskProxyChoiceCustom,
  };

  /// 可选项：带可选副文本（如系统代理摘要 / 全局手动代理 URL）。
  Widget _option(AppColors c, TaskProxyChoice choice, String label, String sub) {
    return ShadOption(
      value: choice,
      child: sub.isEmpty
          ? Text(label)
          : Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(label),
                Text(
                  sub,
                  style: TextStyle(fontSize: 10.5, color: c.textMuted),
                ),
              ],
            ),
    );
  }

  /// 禁用项：非 ShadOption 的普通行（不可选中），灰置 + 原因副文本。
  Widget _disabledOption(AppColors c, String label, String reason) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(label, style: TextStyle(color: c.textMuted)),
          Text(
            reason,
            style: TextStyle(fontSize: 10.5, color: c.textMuted),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final s = LocaleScope.of(context);
    final c = AppColors.of(context);
    final manualConfigured = manualProxyUrl.isNotEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ShadSelect<TaskProxyChoice>(
          initialValue: value,
          options: [
            _option(c, TaskProxyChoice.followGlobal, s.taskProxyChoiceFollow, ''),
            _option(c, TaskProxyChoice.direct, s.taskProxyChoiceDirect, ''),
            if (systemProxyDetected)
              _option(
                c,
                TaskProxyChoice.system,
                s.taskProxyChoiceSystem,
                systemProxySummary,
              )
            else
              _disabledOption(
                c,
                s.taskProxyChoiceSystem,
                s.proxySystemNotDetected,
              ),
            if (manualConfigured)
              _option(
                c,
                TaskProxyChoice.globalManual,
                s.taskProxyChoiceGlobalManual,
                manualProxyUrl,
              )
            else
              _disabledOption(
                c,
                s.taskProxyChoiceGlobalManual,
                s.proxyNotConfigured,
              ),
            _option(c, TaskProxyChoice.custom, s.taskProxyChoiceCustom, ''),
          ],
          selectedOptionBuilder: (context, v) => Text(
            _label(s, v),
            overflow: TextOverflow.ellipsis,
            maxLines: 1,
          ),
          onChanged: (v) {
            if (v == null) return;
            onChanged(v);
          },
        ),
        if (value == TaskProxyChoice.custom) ...[
          const SizedBox(height: 6),
          ShadInput(
            controller: customController,
            placeholder: Text(s.taskProxyPlaceholder),
          ),
        ],
      ],
    );
  }
}
