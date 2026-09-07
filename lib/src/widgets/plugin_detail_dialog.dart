// 插件详情对话框：已安装插件与市场插件共用。
//
// 展示 manifest 级基础信息（名称/版本/作者/标识/主页/标签/发布时间/最低应用
// 版本）、完整描述与通用使用须知。数据全部来自已有信号字段，无新增网络请求。

import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';
import 'package:url_launcher/url_launcher.dart';

import '../i18n/locale_provider.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';

/// 弹出插件详情对话框。已安装插件与市场条目在调用侧拆出各自字段传入。
void showPluginDetailDialog(
  BuildContext context, {
  required String name,
  required String version,
  required String identity,
  required String description,
  required String homepage,
  String author = '',
  List<String> tags = const [],
  String publishTime = '',
  String minAppVersion = '',
  int settingsCount = 0,
  List<String> permissions = const [],
  String? yankedLabel,
}) {
  showShadDialog(
    context: context,
    animateIn: const [],
    animateOut: const [],
    builder: (ctx) {
      final s = currentS;
      final c = AppColors.of(ctx);
      final m = AppMetrics.of(ctx);
      return ShadDialog(
        title: Row(
          children: [
            Flexible(child: Text(name.isNotEmpty ? name : identity)),
            const SizedBox(width: 8),
            Text(
              'v$version',
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w400,
                color: c.textMuted,
              ),
            ),
            if (yankedLabel != null) ...[
              const SizedBox(width: 8),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
                decoration: BoxDecoration(
                  color: m.subtle(AppColors.red),
                  borderRadius: m.brPill,
                ),
                child: Text(
                  yankedLabel,
                  style: TextStyle(
                    fontSize: 10.5,
                    fontWeight: FontWeight.w500,
                    color: AppColors.red,
                  ),
                ),
              ),
            ],
          ],
        ),
        actions: [
          ShadButton.outline(
            onPressed: () => Navigator.of(ctx).pop(),
            child: Text(s.close),
          ),
        ],
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440, maxHeight: 420),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const SizedBox(height: 4),
                _InfoRow(label: s.pluginDetailIdentity, value: identity),
                if (author.isNotEmpty)
                  _InfoRow(label: s.pluginDetailAuthor, value: author),
                if (publishTime.isNotEmpty)
                  _InfoRow(
                    label: s.pluginDetailPublishTime,
                    value: publishTime,
                  ),
                if (minAppVersion.isNotEmpty)
                  _InfoRow(
                    label: s.pluginDetailMinAppVersion,
                    value: minAppVersion,
                  ),
                if (settingsCount > 0)
                  _InfoRow(
                    label: s.pluginDetailSettings,
                    value: s.pluginDetailSettingsCount(settingsCount),
                  ),
                if (homepage.isNotEmpty)
                  _InfoRow(
                    label: s.pluginDetailHomepage,
                    value: homepage,
                    link: true,
                  ),
                if (tags.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 6,
                    runSpacing: 4,
                    children: [
                      for (final tag in tags)
                        Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 7,
                            vertical: 2,
                          ),
                          decoration: BoxDecoration(
                            color: c.surface2,
                            borderRadius: m.brPill,
                          ),
                          child: Text(
                            tag,
                            style: TextStyle(
                              fontSize: 10.5,
                              color: c.textSecondary,
                            ),
                          ),
                        ),
                    ],
                  ),
                ],
                if (permissions.isNotEmpty) ...[
                  const SizedBox(height: 12),
                  _SectionTitle(text: s.pluginDetailPermissions),
                  const SizedBox(height: 4),
                  for (final perm in permissions) _PermissionRow(perm: perm),
                ],
                if (description.isNotEmpty) ...[
                  const SizedBox(height: 12),
                  _SectionTitle(text: s.pluginDetailDescription),
                  const SizedBox(height: 4),
                  Text(
                    description,
                    style: TextStyle(
                      fontSize: 12.5,
                      height: 1.5,
                      color: c.textSecondary,
                    ),
                  ),
                ],
                const SizedBox(height: 12),
                _SectionTitle(text: s.pluginDetailUsage),
                const SizedBox(height: 4),
                Text(
                  s.pluginDetailUsageBody,
                  style: TextStyle(
                    fontSize: 12.5,
                    height: 1.5,
                    color: c.textSecondary,
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    },
  );
}

/// 单条权限行：盾徽 + 权限名 + 说明。权限 → 展示文案在此闭合映射，
/// 新增能力权限时补一臂即可（未知权限降级展示原始名，向前兼容旧客户端
/// 看新插件的场景）。
class _PermissionRow extends StatelessWidget {
  final String perm;

  const _PermissionRow({required this.perm});

  @override
  Widget build(BuildContext context) {
    final s = currentS;
    final c = AppColors.of(context);
    final (name, desc) = switch (perm) {
      'ffmpeg' => (s.pluginPermFfmpegName, s.pluginPermFfmpegDesc),
      'ytdlp' => (s.pluginPermYtdlpName, s.pluginPermYtdlpDesc),
      _ => (perm, s.pluginPermUnknownDesc),
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Icon(LucideIcons.shieldCheck, size: 13, color: c.accent),
          ),
          const SizedBox(width: 6),
          Expanded(
            child: Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: name,
                    style: TextStyle(
                      fontSize: 12.5,
                      fontWeight: FontWeight.w600,
                      color: c.textPrimary,
                    ),
                  ),
                  TextSpan(
                    text: ' — $desc',
                    style: TextStyle(
                      fontSize: 12.5,
                      height: 1.5,
                      color: c.textSecondary,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _SectionTitle extends StatelessWidget {
  final String text;

  const _SectionTitle({required this.text});

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Text(
      text,
      style: TextStyle(
        fontSize: 12.5,
        fontWeight: FontWeight.w600,
        color: c.textPrimary,
      ),
    );
  }
}

class _InfoRow extends StatelessWidget {
  final String label;
  final String value;
  final bool link;

  const _InfoRow({required this.label, required this.value, this.link = false});

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 96,
            child: Text(
              label,
              style: TextStyle(fontSize: 12, color: c.textMuted),
            ),
          ),
          Expanded(
            child: link
                ? GestureDetector(
                    onTap: () => launchUrl(Uri.parse(value)),
                    child: MouseRegion(
                      cursor: SystemMouseCursors.click,
                      child: Text(
                        value,
                        style: TextStyle(fontSize: 12, color: c.accent),
                      ),
                    ),
                  )
                : Text(
                    value,
                    style: TextStyle(fontSize: 12, color: c.textPrimary),
                  ),
          ),
        ],
      ),
    );
  }
}
