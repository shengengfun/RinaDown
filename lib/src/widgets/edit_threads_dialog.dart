/// 「修改线程数」对话框 — 详情面板与任务右键菜单共用。
///
/// 选择新线程数后调用 [DownloadController.setTaskSegments]；引擎侧对活跃任务
/// 自动「暂停 → 改配置 → 恢复」，让新线程数立即生效且已下进度完整保留，
/// 用户无需手动暂停/继续。结果 toast 由 controller.onSegmentsUpdateResult 触发。
library;

import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../i18n/locale_provider.dart';
import '../models/download_controller.dart';
import '../models/download_task.dart';
import '../theme/app_colors.dart';
import 'thread_selector.dart';

/// 打开「修改线程数」对话框。[task] 已完成时不应调用（无意义）。
void showEditThreadsDialog(
  BuildContext context,
  DownloadController controller,
  DownloadTask task,
) {
  final c = AppColors.of(context);
  final s = currentS;
  // ThreadSelector 值语义：null/'auto' = 自动，数字字符串 = 固定线程数。
  String? selected = task.configuredSegments > 0
      ? task.configuredSegments.toString()
      : null;
  showShadDialog(
    context: context,
    barrierColor: c.dialogBarrier,
    builder: (ctx) => ShadDialog(
      title: Text(s.editThreadsTitle),
      actions: [
        ShadButton.outline(
          onPressed: () => Navigator.of(ctx).pop(),
          child: Text(s.cancel),
        ),
        ShadButton(
          onPressed: () {
            final parsed = int.tryParse(selected ?? '') ?? 0;
            final segments = parsed > 0 ? parsed.clamp(1, 256) : 0;
            controller.setTaskSegments(task.id, segments);
            Navigator.of(ctx).pop();
          },
          child: Text(s.confirm),
        ),
      ],
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 12),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ThreadSelector(value: selected, onChanged: (v) => selected = v),
            const SizedBox(height: 12),
            Text(
              s.editThreadsResetHint,
              style: TextStyle(fontSize: 11, color: c.textMuted),
            ),
          ],
        ),
      ),
    ),
  );
}
