import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../i18n/locale_provider.dart';
import '../services/update_service.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';
import 'markdown_body.dart';

/// Show the update changelog dialog with a timeline of new releases.
void showUpdateChangelogDialog(
  BuildContext context, {
  required List<ChangelogRelease> releases,
  required String latestVersion,
  required String currentVersion,
  required VoidCallback onUpdate,
  required VoidCallback onLater,
}) {
  showShadDialog(
    context: context,
    barrierColor: AppColors.of(context).dialogBarrier,
    animateIn: const [],
    animateOut: const [],
    builder: (context) => _ChangelogDialogContent(
      releases: releases,
      latestVersion: latestVersion,
      currentVersion: currentVersion,
      onUpdate: onUpdate,
      onLater: onLater,
    ),
  );
}

class _ChangelogDialogContent extends StatelessWidget {
  final List<ChangelogRelease> releases;
  final String latestVersion;
  final String currentVersion;
  final VoidCallback onUpdate;
  final VoidCallback onLater;

  const _ChangelogDialogContent({
    required this.releases,
    required this.latestVersion,
    required this.currentVersion,
    required this.onUpdate,
    required this.onLater,
  });

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final s = LocaleScope.of(context);
    final m = AppMetrics.of(context);

    return ShadDialog(
      constraints: const BoxConstraints(maxWidth: 520),
      title: Row(
        children: [
          Container(
            width: 28,
            height: 28,
            decoration: BoxDecoration(
              color: m.soft(c.accent),
              borderRadius: m.brMd,
            ),
            child: Icon(LucideIcons.sparkles, size: 14, color: c.accent),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(s.changelogTitle, overflow: TextOverflow.ellipsis),
          ),
          if (releases.length > 1)
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
              decoration: BoxDecoration(
                color: c.accentBg,
                borderRadius: m.brDialog,
              ),
              child: Text(
                s.changelogVersionCount(releases.length),
                style: TextStyle(fontSize: 10, color: c.accent),
              ),
            ),
        ],
      ),
      description: Text(s.changelogSubtitle(latestVersion)),
      actions: [
        ShadButton.outline(
          onPressed: () {
            Navigator.of(context).pop();
            onLater();
          },
          child: Text(s.changelogLater),
        ),
        ShadButton(
          onPressed: () {
            Navigator.of(context).pop();
            onUpdate();
          },
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(LucideIcons.download, size: 13),
              const SizedBox(width: 6),
              Text(s.changelogUpdateNow),
            ],
          ),
        ),
      ],
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 400),
        child: ScrollConfiguration(
          behavior: ScrollConfiguration.of(context).copyWith(scrollbars: true),
          child: SingleChildScrollView(
            physics: const ClampingScrollPhysics(),
            padding: const EdgeInsets.symmetric(vertical: 12),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (int i = 0; i < releases.length; i++) ...[
                  if (i > 0)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 8),
                      child: Container(height: 1, color: c.border),
                    ),
                  _ReleaseEntry(
                    release: releases[i],
                    isFirst: i == 0,
                    isLast: i == releases.length - 1,
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

// ─────────────────────────────────────────────
// Bilingual release body
// ─────────────────────────────────────────────

/// 双语 release body 的语言标记（由 release 工作流翻译步骤写入），
/// 与官网 ChangelogSection.tsx 的解析契约一致。
final _langMarkerRe = RegExp(r'<!--\s*fluxdown:lang:(zh|en)\s*-->');

/// 从双语 release body 中取出当前语言区块。
/// 无标记（历史版本 / 翻译失败回退）时原样返回全文。
String pickLocaleBody(String body, String locale) {
  final matches = _langMarkerRe.allMatches(body).toList();
  if (matches.isEmpty) return body;

  final sections = <String, String>{};
  for (var i = 0; i < matches.length; i++) {
    final start = matches[i].end;
    final end = i + 1 < matches.length ? matches[i + 1].start : body.length;
    sections[matches[i].group(1)!] = body.substring(start, end).trim();
  }
  final lang = locale.startsWith('zh') ? 'zh' : 'en';
  return sections[lang] ?? sections['zh'] ?? body;
}

// ─────────────────────────────────────────────
// Single release entry
// ─────────────────────────────────────────────

class _ReleaseEntry extends StatelessWidget {
  final ChangelogRelease release;
  final bool isFirst;
  final bool isLast;

  const _ReleaseEntry({
    required this.release,
    required this.isFirst,
    required this.isLast,
  });

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);

    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Timeline column
        SizedBox(
          width: 20,
          child: Column(
            children: [
              const SizedBox(height: 3),
              Container(
                width: 10,
                height: 10,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: isFirst ? c.accent : c.bg,
                  border: Border.all(
                    color: isFirst ? c.accent : c.border,
                    width: 1.5,
                  ),
                ),
              ),
            ],
          ),
        ),
        const SizedBox(width: 8),
        // Content column
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              // Version tag + date
              Wrap(
                spacing: 8,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 7,
                      vertical: 1.5,
                    ),
                    decoration: BoxDecoration(
                      color: c.accentBg,
                    borderRadius: m.brSm,
                    ),
                    child: Text(
                      release.tag,
                      style: TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.w600,
                        color: c.accent,
                      ),
                    ),
                  ),
                  if (release.publishedAt.isNotEmpty)
                    Text(
                      _formatDate(release.publishedAt),
                      style: TextStyle(fontSize: 11, color: c.textMuted),
                    ),
                  if (release.publishedAt.isNotEmpty)
                    Text(
                      _relativeTime(release.publishedAt),
                      style: TextStyle(fontSize: 11, color: c.textMuted),
                    ),
                ],
              ),
              const SizedBox(height: 8),
              // Markdown body
              if (release.body.isNotEmpty)
                MarkdownBody(
                  markdown: pickLocaleBody(release.body, currentLocale),
                ),
            ],
          ),
        ),
      ],
    );
  }

  String _formatDate(String isoDate) {
    try {
      final dt = DateTime.parse(isoDate);
      final locale = currentLocale;
      if (locale.startsWith('zh')) {
        return '${dt.year}年${dt.month}月${dt.day}日';
      }
      const months = [
        'Jan',
        'Feb',
        'Mar',
        'Apr',
        'May',
        'Jun',
        'Jul',
        'Aug',
        'Sep',
        'Oct',
        'Nov',
        'Dec',
      ];
      return '${months[dt.month - 1]} ${dt.day}, ${dt.year}';
    } catch (_) {
      return isoDate;
    }
  }

  String _relativeTime(String isoDate) {
    try {
      final dt = DateTime.parse(isoDate);
      final now = DateTime.now();
      final days = now.difference(dt).inDays;
      final locale = currentLocale;
      final isZh = locale.startsWith('zh');

      if (days == 0) return isZh ? '今天' : 'today';
      if (days == 1) return isZh ? '昨天' : 'yesterday';
      if (days < 30) return isZh ? '$days 天前' : '$days days ago';
      final months = days ~/ 30;
      if (months < 12) {
        return isZh ? '$months 个月前' : '$months months ago';
      }
      final years = days ~/ 365;
      return isZh ? '$years 年前' : '$years years ago';
    } catch (_) {
      return '';
    }
  }
}
