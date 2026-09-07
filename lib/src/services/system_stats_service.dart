// 系统性能监控数据层（Windows，Rust 每 ~2s 推送 SystemStats）。
// 状态栏「限速」左侧的 CPU/内存/网络芯片消费本服务。
//
// 用法：HomePage.initState 调一次 [start]；UI 用 ListenableBuilder 包本服务。

import 'dart:async';

import 'package:flutter/foundation.dart';

import '../bindings/bindings.dart';
import 'log_service.dart';

const _tag = 'SystemStats';

class SystemStatsService extends ChangeNotifier {
  SystemStatsService._();
  static final SystemStatsService instance = SystemStatsService._();

  StreamSubscription<dynamic>? _sub;

  /// 是否有过采样（Windows 才有；其它平台恒 false → UI 隐藏）。
  bool get active => _hasData;

  bool _hasData = false;
  double _cpu = 0;
  double _ram = 0;
  int _downBps = 0;
  int _upBps = 0;

  double get cpuPercent => _cpu;
  double get ramPercent => _ram;
  int get netDownBps => _downBps;
  int get netUpBps => _upBps;

  void start() {
    if (_sub != null) return;
    _sub = SystemStats.rustSignalStream.listen((pack) {
      final m = pack.message;
      _hasData = true;
      _cpu = m.cpuPercent;
      _ram = m.ramPercent;
      _downBps = m.netDownBps.toInt();
      _upBps = m.netUpBps.toInt();
      notifyListeners();
    }, onError: (Object e) {
      logError(_tag, 'stream error: $e');
    });
    logInfo(_tag, 'started');
  }

  @override
  void dispose() {
    _sub?.cancel();
    _sub = null;
    super.dispose();
  }
}
