import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';
import '../i18n/locale_provider.dart';
import '../models/download_controller.dart';
import '../models/download_task.dart';
import '../theme/app_colors.dart';

/// 做种总览汇总条 —— 仅在主列表处于「做种」筛选时渲染于列表顶部。
/// 聚合全部任务集合：正在做种数、排队做种数、总上传速度、累计上传量。
class SeedingSummaryBar extends StatelessWidget {
  final DownloadController controller;

  const SeedingSummaryBar({super.key, required this.controller});

  /// 字节/秒 → 可读速率（整数不显示小数，与 status_bar 展示口径一致）。
  static String _formatSpeed(int bytes) {
    if (bytes >= 1024 * 1024) {
      final mb = bytes / (1024 * 1024);
      final rounded = mb.round();
      return rounded == mb ? '$rounded MB/s' : '${mb.toStringAsFixed(1)} MB/s';
    }
    return '${(bytes / 1024).round()} KB/s';
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final s = LocaleScope.of(context);
    var active = 0;
    var queued = 0;
    var uploadSpeed = 0;
    var uploadedTotal = 0;
    for (final t in controller.tasks) {
      if (t.seedingStatus == SeedingStatus.seeding) {
        active++;
        uploadSpeed += t.uploadSpeedBps;
        uploadedTotal += t.uploadedBytes;
      } else if (t.seedingStatus == SeedingStatus.queued) {
        queued++;
        uploadedTotal += t.uploadedBytes;
      }
    }

    return Container(
      height: 32,
      padding: const EdgeInsets.symmetric(horizontal: 16),
      decoration: BoxDecoration(
        color: c.surface1,
        border: Border(bottom: BorderSide(color: c.border, width: 1)),
      ),
      child: Row(
        children: [
          Icon(LucideIcons.arrowUpCircle, size: 13, color: c.textMuted),
          const SizedBox(width: 6),
          _Item(text: s.seedingSummaryActive(active)),
          _Separator(color: c.border),
          _Item(text: s.seedingSummaryQueued(queued)),
          _Separator(color: c.border),
          Icon(LucideIcons.arrowUp, size: 12, color: c.textMuted),
          const SizedBox(width: 4),
          _Item(text: _formatSpeed(uploadSpeed)),
          _Separator(color: c.border),
          _Item(
            text:
                '${s.uploadedTotal} ${DownloadTask.formatBytes(uploadedTotal)}',
          ),
        ],
      ),
    );
  }
}

class _Item extends StatelessWidget {
  final String text;

  const _Item({required this.text});

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Text(
      text,
      style: TextStyle(
        fontSize: 11,
        color: c.textSecondary,
        fontFeatures: const [FontFeature.tabularFigures()],
      ),
    );
  }
}

class _Separator extends StatelessWidget {
  final Color color;

  const _Separator({required this.color});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 1,
      height: 12,
      margin: const EdgeInsets.symmetric(horizontal: 10),
      color: color,
    );
  }
}
