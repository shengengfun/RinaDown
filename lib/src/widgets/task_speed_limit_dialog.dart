// 单个任务「下载限速」设置对话框（详情页与右键菜单共用）。
// 单位跟随设置页「下载限速」上方的限速单位；0 = 不限（跟随队列/全局）。

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../i18n/locale_provider.dart';
import '../models/download_controller.dart';
import '../models/download_task.dart';
import '../models/settings_provider.dart';
import '../models/speed_unit.dart';
import '../theme/app_colors.dart';

/// 弹出单任务下载限速设置。正在运行的任务立即生效（引擎侧分层限速器）。
Future<void> showTaskSpeedLimitDialog(
  BuildContext context,
  DownloadController controller,
  DownloadTask task,
) async {
  final s = LocaleScope.of(context);
  final c = AppColors.of(context);
  final unit =
      SettingsProvider.globalInstance?.speedLimitUnit ??
      kDefaultSpeedLimitUnit;
  final currentBps = controller.taskSpeedLimitBps(task.id);
  final ctrl = TextEditingController(
    text: currentBps > 0 ? '${valueFromBytes(unit, currentBps)}' : '0',
  );
  var resultBps = currentBps;
  await showShadDialog<void>(
    context: context,
    barrierColor: c.dialogBarrier,
    builder: (dialogContext) => ShadDialog(
      // 弹窗只承载「一个数字 + 单位」，宽度收紧到刚好容纳一行，
      // 避免输入框横跨整个对话框（原实现用 Expanded 撑满）。
      constraints: const BoxConstraints(maxWidth: 360),
      title: Text(s.speedLimit),
      actions: [
        ShadButton.outline(
          onPressed: () {
            resultBps = 0;
            Navigator.of(dialogContext).pop();
          },
          child: Text(s.statusSpeedLimitOff),
        ),
        ShadButton(
          onPressed: () {
            final v = int.tryParse(ctrl.text.trim()) ?? 0;
            resultBps = bytesFromValue(unit, v);
            Navigator.of(dialogContext).pop();
          },
          child: Text(s.confirm),
        ),
      ],
      child: Padding(
        padding: const EdgeInsets.only(top: 8),
        child: Row(
          children: [
            SizedBox(
              width: 132,
              child: ShadInput(
                controller: ctrl,
                keyboardType: TextInputType.number,
                inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                placeholder: const Text('0'),
                onSubmitted: (_) => Navigator.of(dialogContext).pop(),
              ),
            ),
            const SizedBox(width: 8),
            Text(unit, style: TextStyle(fontSize: 12, color: c.textMuted)),
          ],
        ),
      ),
    ),
  );
  ctrl.dispose();
  if (resultBps != currentBps) {
    controller.setTaskSpeedLimit(task.id, resultBps);
  }
}
