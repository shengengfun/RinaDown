// 内置浏览器（应用内）：输入网页地址 → 应用自己抓取该页 HTML → 列出页面里
// 可直接下载的链接（文件后缀命中 / <video>/<audio>/<source> 媒体 / 带 download
// 属性的 <a>）→ 一键「接管」，直接在本应用建任务，不再经过外部浏览器。
//
// 设计取舍（见 AGENTS.md「禁止新增 dependency」）：不引入 WebView 引擎（需新增
// 依赖），改为「下载链接浏览器」——只取页面里对下载管理器有意义的信息，抓取走
// dart:io HttpClient，页面正文不渲染。命中后缀白名单或媒体标签的链接才会列出，
// 避免把整页导航链接都塞进列表。
//
// 相对链接按页面 URL 解析为绝对地址；仅接受 http/https，去重后按原页面顺序展示。

import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../bindings/bindings.dart';
import '../i18n/locale_provider.dart';
import '../models/settings_provider.dart';
import '../services/file_picker_service.dart';
import '../services/recent_dirs.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';
import 'dir_picker_field.dart';

/// 打开内置浏览器对话框。可选 [initialUrl] 预填地址（剪贴板嗅探入口）。
void showBuiltinBrowserDialog(BuildContext context, {String initialUrl = ''}) {
  showShadDialog(
    context: context,
    barrierColor: AppColors.of(context).dialogBarrier,
    animateIn: const [],
    animateOut: const [],
    builder: (_) => _BuiltinBrowserDialogContent(initialUrl: initialUrl),
  );
}

/// 视为「可直接下载」的文件后缀（小写、不含点）。
const Set<String> _downloadableExts = {
  'zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz', 'zst', 'iso', 'img',
  'exe', 'msi', 'msix', 'apk', 'ipa', 'dmg', 'pkg', 'deb', 'rpm', 'appimage',
  'pdf', 'epub', 'mobi', 'azw3', 'djvu',
  'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'ods', 'odp',
  'mp3', 'flac', 'wav', 'aac', 'm4a', 'ogg', 'opus', 'wma',
  'mp4', 'mkv', 'webm', 'avi', 'mov', 'flv', 'wmv', 'm4v', 'ts',
  'bin', 'dat', 'jar', 'sql', 'db', 'csv', 'json', 'xml', 'txt', 'torrent',
};

/// 页面里的一条候选下载链接。
class _PageLink {
  final String url;
  final String name;
  final bool fromMediaTag;

  const _PageLink({
    required this.url,
    required this.name,
    required this.fromMediaTag,
  });

  String get host => Uri.tryParse(url)?.host ?? '';
}

/// 百分号解码，失败时原样返回（页面里的非法百分号不该让整个解析失败）。
String _decode(String raw) {
  try {
    return Uri.decodeComponent(raw);
  } catch (_) {
    return raw;
  }
}

class _BuiltinBrowserDialogContent extends StatefulWidget {
  final String initialUrl;

  const _BuiltinBrowserDialogContent({this.initialUrl = ''});

  @override
  State<_BuiltinBrowserDialogContent> createState() =>
      _BuiltinBrowserDialogContentState();
}

class _BuiltinBrowserDialogContentState
    extends State<_BuiltinBrowserDialogContent> {
  final _url = TextEditingController();
  final _saveDir = TextEditingController();

  List<_PageLink> _links = const [];
  String _pageTitle = '';
  String _error = '';
  bool _loading = false;
  bool _fetched = false;

  @override
  void initState() {
    super.initState();
    final u = widget.initialUrl.trim();
    if (u.isNotEmpty) _url.text = u;
    final settings = SettingsProvider.globalInstance;
    if (settings != null) {
      _saveDir.text = settings.effectiveDefaultSaveDir;
    }
  }

  @override
  void dispose() {
    _url.dispose();
    _saveDir.dispose();
    super.dispose();
  }

  Future<void> _pickDir() async {
    final selected = await FilePickerService.pickDirectory(
      dialogTitle: LocaleScope.of(context).selectSaveDir,
    );
    if (selected != null && mounted) {
      setState(() => _saveDir.text = selected);
    }
  }

  Future<void> _open() async {
    final raw = _url.text.trim();
    if (raw.isEmpty || _loading) return;
    var input = raw;
    if (!input.contains('://')) input = 'https://$input';
    final Uri uri;
    try {
      uri = Uri.parse(input);
    } catch (_) {
      setState(() => _error = LocaleScope.of(context).builtinBrowserFetchFailed);
      return;
    }
    if (uri.scheme != 'http' && uri.scheme != 'https') {
      setState(() => _error = LocaleScope.of(context).builtinBrowserFetchFailed);
      return;
    }
    setState(() {
      _loading = true;
      _error = '';
      _links = const [];
      _pageTitle = '';
      _fetched = false;
    });
    try {
      final client = HttpClient()
        ..connectionTimeout = const Duration(seconds: 20)
        ..userAgent = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) '
            'AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36';
      final request = await client.getUrl(uri);
      request.headers.set(
        HttpHeaders.acceptHeader,
        'text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8',
      );
      final response = await request.close();
      if (response.statusCode >= 400) {
        throw HttpException('HTTP ${response.statusCode}');
      }
      final body = await response.transform(utf8.decoder).join();
      client.close(force: true);
      final parsed = _parseLinks(body, uri);
      if (!mounted) return;
      setState(() {
        _loading = false;
        _fetched = true;
        _pageTitle = parsed.title;
        _links = parsed.links;
      });
    } catch (e) {
      if (!mounted) return;
      final s = LocaleScope.of(context);
      setState(() {
        _loading = false;
        _fetched = true;
        _error = '${s.builtinBrowserFetchFailed}：$e';
      });
    }
  }

  /// 从 HTML 文本提取候选下载链接。
  ({String title, List<_PageLink> links}) _parseLinks(String html, Uri base) {
    final titleMatch =
        RegExp(r'<title[^>]*>([\s\S]*?)</title>', caseSensitive: false)
            .firstMatch(html);
    final title =
        (titleMatch?.group(1) ?? '').replaceAll(RegExp(r'\s+'), ' ').trim();

    final seen = <String>{};
    final links = <_PageLink>[];

    void add(String rawHref, {required bool media}) {
      final ref = rawHref.trim();
      if (ref.isEmpty) return;
      final lower = ref.toLowerCase();
      if (lower.startsWith('javascript:') ||
          lower.startsWith('mailto:') ||
          lower.startsWith('tel:') ||
          lower.startsWith('data:') ||
          lower.startsWith('#')) {
        return;
      }
      final Uri resolved;
      try {
        resolved = base.resolve(ref);
      } catch (_) {
        return;
      }
      if (resolved.scheme != 'http' && resolved.scheme != 'https') return;
      final url = resolved.toString();
      if (!seen.add(url)) return;
      final path = resolved.path;
      final dot = path.lastIndexOf('.');
      final ext = dot >= 0 && dot < path.length - 1
          ? path.substring(dot + 1).toLowerCase()
          : '';
      if (!media && !_downloadableExts.contains(ext)) return;
      var name = _decode(
        path.isEmpty ? resolved.host : path.substring(path.lastIndexOf('/') + 1),
      );
      if (name.trim().isEmpty) name = resolved.host;
      links.add(_PageLink(url: url, name: name, fromMediaTag: media));
    }

    // 媒体标签：<video|audio|source|embed|track src=...> 无条件收录。
    final mediaRe = RegExp(
      r'''<(?:video|audio|source|embed|track)[^>]*\bsrc\s*=\s*["']([^"']+)["']''',
      caseSensitive: false,
    );
    for (final m in mediaRe.allMatches(html)) {
      add(m.group(1) ?? '', media: true);
    }
    // <a href>：带 download 属性的一律收录，其余看后缀白名单。
    final anchorRe = RegExp(r'<a\b[^>]*>[\s\S]*?</a>', caseSensitive: false);
    final hrefRe =
        RegExp(r'''\bhref\s*=\s*["']([^"']+)["']''', caseSensitive: false);
    for (final m in anchorRe.allMatches(html)) {
      final tag = m.group(0) ?? '';
      final href = hrefRe.firstMatch(tag)?.group(1);
      if (href == null) continue;
      final hasDownload = RegExp(
        r'\bdownload(?:\s*=|[\s>/])',
        caseSensitive: false,
      ).hasMatch(tag);
      add(href, media: hasDownload);
    }

    return (title: title, links: links);
  }

  void _takeover(String url) {
    final dir = _saveDir.text.trim();
    if (dir.isNotEmpty) RecentDirs.instance.push(dir);
    CreateTask(
      url: url,
      saveDir: dir,
      fileName: '',
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
      resolverItem: '',
    ).sendSignalToRust();
  }

  void _takeoverAll() {
    if (_links.isEmpty) return;
    _takeover(_links.map((l) => l.url).join('\n'));
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    final s = LocaleScope.of(context);
    return ShadDialog(
      constraints: const BoxConstraints(maxWidth: 560),
      title: Row(
        children: [
          Icon(LucideIcons.globe, color: c.accent, size: 18),
          const SizedBox(width: 8),
          Text(s.builtinBrowser),
        ],
      ),
      description: Text(s.builtinBrowserHint),
      actions: [
        ShadButton.outline(
          onPressed: _loading ? null : () => Navigator.of(context).pop(),
          child: Text(s.cancel),
        ),
        ShadButton(
          onPressed: _links.isEmpty || _loading ? null : _takeoverAll,
          child: Text(
            s.builtinBrowserDownloadAll,
            style: const TextStyle(color: Color(0xFFFFFFFF)),
          ),
        ),
      ],
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: ShadInput(
                  controller: _url,
                  placeholder: Text(s.builtinBrowserUrlPlaceholder),
                ),
              ),
              const SizedBox(width: 8),
              ShadButton(
                onPressed: _loading ? null : _open,
                child: Text(
                  _loading ? s.builtinBrowserLoading : s.builtinBrowserOpen,
                ),
              ),
            ],
          ),
          const SizedBox(height: 8),
          DirPickerField(
            path: _saveDir.text,
            placeholder: s.selectSaveDir,
            enabled: !_loading,
            onTap: _pickDir,
            onPathSelected: (dir) {
              setState(() => _saveDir.text = dir);
              RecentDirs.instance.push(dir);
            },
          ),
          if (_error.isNotEmpty) ...[
            const SizedBox(height: 10),
            SelectableText(
              _error,
              style: TextStyle(color: c.textMuted, fontSize: 12),
            ),
          ],
          if (_fetched && _links.isEmpty && _error.isEmpty) ...[
            const SizedBox(height: 14),
            Text(
              s.builtinBrowserNoLinks,
              style: TextStyle(color: c.textMuted, fontSize: 12.5),
            ),
          ],
          if (_links.isNotEmpty) ...[
            const SizedBox(height: 14),
            Text(
              _pageTitle.isEmpty ? _url.text.trim() : _pageTitle,
              style: TextStyle(
                color: c.textPrimary,
                fontWeight: FontWeight.w600,
              ),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            const SizedBox(height: 8),
            ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 300),
              child: SingleChildScrollView(
                child: Column(
                  children: [
                    for (final link in _links)
                      Container(
                        margin: const EdgeInsets.only(bottom: 6),
                        padding: const EdgeInsets.all(10),
                        decoration: BoxDecoration(
                          color: c.surface1,
                          borderRadius: m.brCard,
                          border: Border.all(color: c.border),
                        ),
                        child: Row(
                          children: [
                            Icon(
                              link.fromMediaTag
                                  ? LucideIcons.play
                                  : LucideIcons.fileDown,
                              size: 16,
                              color: c.accent,
                            ),
                            const SizedBox(width: 8),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Text(
                                    link.name,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(
                                      fontSize: 12.5,
                                      color: c.textPrimary,
                                    ),
                                  ),
                                  if (link.host.isNotEmpty)
                                    Text(
                                      link.host,
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                      style: TextStyle(
                                        fontSize: 10.5,
                                        color: c.textMuted,
                                      ),
                                    ),
                                ],
                              ),
                            ),
                            const SizedBox(width: 8),
                            ShadButton.ghost(
                              onPressed: () => _takeover(link.url),
                              child: Text(
                                s.builtinBrowserTakeover,
                                style: TextStyle(fontSize: 12, color: c.accent),
                              ),
                            ),
                          ],
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
