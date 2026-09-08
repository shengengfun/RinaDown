// RinaDown 剪贴板监听服务（桌面 Windows/macOS）。
//
// 作用：定时读取剪贴板，命中「已启用平台」的 http(s) 链接时，弹一个小窗
// 询问是否解析该音视频（解析 → 视频解析对话框预填；直接下载 → 快速下载
// 对话框；忽略 → 该平台静默一段时间）。
//
// 设计：
// - 轮询 1.5s（桌面剪贴板全局可读，无 Wayland 失焦门控）；文本无变化跳过。
// - 防骚扰：同一签名只提示一次（FIFO 30 条内存表）；「忽略」对该平台 snooze
//   10 分钟；弹窗打开期间不再触发。
// - 主窗隐藏到托盘时先恢复主窗再弹（提示必然可见），若在忙可随时在设置里关
//   掉主开关。
//
// Linux Wayland 走既有 WaylandDegradationService（主窗恢复时读一次）；本服务
// 只在 Windows/macOS 激活，二者互不干扰（都读同一个 clipboardWatchEnabled
// 主开关）。

import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../i18n/locale_provider.dart';
import '../models/settings_provider.dart';
import '../theme/app_colors.dart';
import '../widgets/quick_download_dialog.dart';
import '../widgets/video_parse_dialog.dart';
import 'clipboard_platforms.dart';
import 'log_service.dart';
import 'tray_service.dart';

const _tag = 'ClipWatch';

/// 平台「忽略」后静默时长。
const _kPlatformSnooze = Duration(minutes: 10);

/// 同一 URL 不重复提示的去重表容量。
const _kSeenLimit = 30;

class ClipboardWatchService {
  ClipboardWatchService._();
  static final ClipboardWatchService instance = ClipboardWatchService._();

  SettingsProvider? _settings;
  GlobalKey<NavigatorState>? _navigatorKey;
  Timer? _timer;
  bool _running = false;
  bool _dialogOpen = false;
  String _lastClipboardText = '';
  final List<String> _seen = [];
  final Map<String, DateTime> _snoozedPlatform = {};

  /// 初始化 — 等配置加载完成后调用（与悬浮球同款钩子）。
  void init({
    required SettingsProvider settings,
    required GlobalKey<NavigatorState> navigatorKey,
  }) {
    if (_settings != null) return; // 幂等
    _settings = settings;
    _navigatorKey = navigatorKey;
    settings.addListener(_syncRunning);
    _syncRunning();
    logInfo(_tag, 'init done, running=$_running');
  }

  bool get isSupported =>
      Platform.isWindows || Platform.isMacOS;

  void _syncRunning() {
    final want = _settings?.clipboardWatchEnabled == true && isSupported;
    if (want == _running) return;
    _running = want;
    if (want) {
      _timer ??= Timer.periodic(
        const Duration(milliseconds: 1500),
        (_) => _poll(),
      );
      logInfo(_tag, 'watcher started');
    } else {
      _timer?.cancel();
      _timer = null;
      _lastClipboardText = '';
      logInfo(_tag, 'watcher stopped');
    }
  }

  Future<void> _poll() async {
    if (!_running || _dialogOpen) return;
    final String text;
    try {
      final data = await Clipboard.getData(Clipboard.kTextPlain);
      text = data?.text ?? '';
    } catch (_) {
      return; // 剪贴板读取偶发失败静默跳过
    }
    if (text.isEmpty || text == _lastClipboardText) return;
    _lastClipboardText = text;

    final matched = matchClipboardPlatformUrl(text);
    if (matched == null) return;
    final (platformId, url) = matched;
    final settings = _settings;
    if (settings == null) return;
    if (!settings.isClipboardPlatformEnabled(platformId)) return;

    final sig = '$platformId|$url';
    if (_seen.contains(sig)) return;
    _rememberSeen(sig);

    final snoozeUntil = _snoozedPlatform[platformId];
    if (snoozeUntil != null && DateTime.now().isBefore(snoozeUntil)) return;

    await _offer(platformId, url);
  }

  void _rememberSeen(String sig) {
    _seen.remove(sig);
    _seen.add(sig);
    if (_seen.length > _kSeenLimit) {
      _seen.removeRange(0, _seen.length - _kSeenLimit);
    }
  }

  void _snooze(String platformId) {
    _snoozedPlatform[platformId] = DateTime.now().add(_kPlatformSnooze);
  }

  /// 弹出「是否解析」小窗：先恢复主窗（若隐藏到托盘），再经根 Navigator 弹窗。
  Future<void> _offer(String platformId, String url) async {
    final ctx = _navigatorKey?.currentContext;
    final settings = _settings;
    if (ctx == null || !ctx.mounted || settings == null) return;
    if (_dialogOpen) return;
    _dialogOpen = true;
    try {
      await restoreMainWindow();
      final ctx2 = _navigatorKey?.currentContext;
      if (ctx2 == null || !ctx2.mounted) return;
      final s = currentS;
      final label = s.clipboardPlatformName(platformId);
      await showShadDialog<void>(
        context: ctx2,
        barrierColor: AppColors.of(ctx2).dialogBarrier,
        builder: (dctx) {
          final c = AppColors.of(dctx);
          return ShadDialog(
            title: Row(
              children: [
                Icon(LucideIcons.video, color: c.accent, size: 18),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    s.clipboardParseOfferTitle(label),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
            description: Text(s.clipboardParseOfferBody(url)),
            actions: [
              ShadButton.outline(
                onPressed: () {
                  Navigator.of(dctx).pop();
                  _snooze(platformId);
                },
                child: Text(s.clipboardIgnoreAction),
              ),
              ShadButton.outline(
                onPressed: () {
                  Navigator.of(dctx).pop();
                  _openQuickDownload(ctx2, url, settings);
                },
                child: Text(s.clipboardQuickAction),
              ),
              ShadButton(
                onPressed: () {
                  Navigator.of(dctx).pop();
                  _openParse(ctx2, url);
                },
                child: Text(
                  s.clipboardParseAction,
                  style: const TextStyle(color: Color(0xFFFFFFFF)),
                ),
              ),
            ],
          );
        },
      );
    } finally {
      _dialogOpen = false;
    }
  }

  void _openParse(BuildContext ctx, String url) {
    showVideoParseDialog(ctx, initialUrl: url);
  }

  void _openQuickDownload(
    BuildContext ctx,
    String url,
    SettingsProvider settings,
  ) {
    showQuickDownloadDialog(
      ctx,
      url: url,
      filename: '',
      fileSize: 0,
      mimeType: '',
      cookies: '',
      defaultSaveDir: settings.effectiveDefaultSaveDir,
      defaultQueueId: settings.defaultQueueId,
    );
  }
}
