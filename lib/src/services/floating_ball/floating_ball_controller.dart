/// 悬浮球数据层（方案 S0.2）。
///
/// 订阅 [DownloadController] 与 [ThemeProvider]，把下载状态聚合为
/// [BallActiveSpec]；仅当**渲染文本实际变化**（速度字符串/角标数/进度桶）
/// 或主题变更时才 notify — 上游 Rust 信号已节流 500ms（A3），
/// 本层再做变化检测，activeCount==0 时零重绘。
library;

import 'package:flutter/foundation.dart';

import '../../models/download_controller.dart';
import '../../models/download_task.dart';
import '../../theme/theme_provider.dart';
import 'floating_ball_renderer.dart';

/// 悬浮球展示状态快照
class BallGlanceState {
  /// 是否有活跃任务（false = idle 变体）
  final bool isActive;

  /// active 态渲染参数（isActive==false 时为 null）
  final BallActiveSpec? activeSpec;

  const BallGlanceState({required this.isActive, this.activeSpec});
}

/// 数据变化 → 渲染参数聚合器。
///
/// 消费方（FloatingBallService）监听本类，收到通知后读 [state] 重渲染。
class FloatingBallController extends ChangeNotifier {
  final DownloadController _downloads;
  final ThemeProvider _theme;

  BallGlanceState _state = const BallGlanceState(isActive: false);
  BallActiveSpec? _lastSpec;
  bool _lastActive = false;
  bool _disposed = false;

  /// 主题代际计数 — 消费方以此作为位图缓存键成分（A7 缓存键）。
  int themeGeneration = 0;

  FloatingBallController({
    required DownloadController downloads,
    required ThemeProvider theme,
  }) : _downloads = downloads,
       _theme = theme {
    _downloads.addListener(_onDownloadsChanged);
    _theme.addListener(_onThemeChanged);
    _onDownloadsChanged();
  }

  BallGlanceState get state => _state;

  void _onDownloadsChanged() {
    if (_disposed) return;
    final active = _downloads.activeCount;
    if (active <= 0) {
      if (_lastActive) {
        _lastActive = false;
        _lastSpec = null;
        _state = const BallGlanceState(isActive: false);
        notifyListeners();
      }
      return;
    }

    final spec = BallActiveSpec(
      speedText: _formatSpeed(_downloads.totalDownloadSpeed),
      activeCount: active,
      aggregateProgress: _aggregateProgress(),
    );

    // 变化检测：文本级相等 → 跳过（含进度按 1% 粒度分桶后的比较）
    if (_lastActive && spec == _lastSpec) return;
    _lastActive = true;
    _lastSpec = spec;
    _state = BallGlanceState(isActive: true, activeSpec: spec);
    notifyListeners();
  }

  void _onThemeChanged() {
    if (_disposed) return;
    themeGeneration++;
    notifyListeners();
  }

  /// 聚合进度 — 字节加权平均（偏好⑤推荐默认）。
  ///
  /// totalBytes<=0（大小未知/BT磁力解析中）的任务不计入；
  /// 分母为 0 → null（环形显示不确定动画，m1 裁决）。
  double? _aggregateProgress() {
    var downloaded = 0;
    var total = 0;
    for (final t in _downloads.tasks) {
      if (t.status != TaskStatus.downloading &&
          t.status != TaskStatus.pending &&
          t.status != TaskStatus.preparing &&
          t.status != TaskStatus.resuming) {
        continue;
      }
      if (t.totalBytes <= 0) continue;
      downloaded += t.downloadedBytes;
      total += t.totalBytes;
    }
    if (total <= 0) return null;
    // 1% 分桶 — 防止逐字节变化触发无意义重绘
    return ((downloaded / total) * 100).floorToDouble() / 100;
  }

  /// 速度格式化（紧凑形式，球体空间有限）
  static String _formatSpeed(int bytesPerSec) {
    if (bytesPerSec <= 0) return '—';
    return '${DownloadTask.formatBytes(bytesPerSec)}/s';
  }

  @override
  void dispose() {
    _disposed = true;
    _downloads.removeListener(_onDownloadsChanged);
    _theme.removeListener(_onThemeChanged);
    super.dispose();
  }
}
