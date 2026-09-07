import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:rinf/rinf.dart';

import '../bindings/bindings.dart';
import '../services/log_service.dart';
import '../widgets/hls_quality_dialog.dart';

const _tag = 'HlsQualitySvc';

class HlsQualityService {
  static HlsQualityService? _instance;

  final GlobalKey<NavigatorState> navigatorKey;
  StreamSubscription<RustSignalPack<HlsQualityOptions>>? _sub;
  bool _dialogOpen = false;

  HlsQualityService._({required this.navigatorKey});

  static void init({required GlobalKey<NavigatorState> navigatorKey}) {
    logInfo(_tag, 'init');
    _instance?._teardown();
    _instance = HlsQualityService._(navigatorKey: navigatorKey);
    _instance!._startListening();
  }

  static void shutdown() {
    logInfo(_tag, 'shutdown');
    _instance?._teardown();
    _instance = null;
  }

  void _teardown() {
    _sub?.cancel();
  }

  void _startListening() {
    _sub = HlsQualityOptions.rustSignalStream.listen(_onQualityOptions);
  }

  void _onQualityOptions(RustSignalPack<HlsQualityOptions> pack) {
    final msg = pack.message;
    logInfo(
      _tag,
      'received quality options: task=${msg.taskId}, count=${msg.options.length}',
    );

    if (_dialogOpen) {
      logInfo(_tag, 'dialog already open, ignoring');
      return;
    }

    final context = navigatorKey.currentContext;
    if (context == null) {
      logInfo(_tag, 'no context, auto-selecting best quality');
      _autoSelectBest(msg);
      return;
    }

    if (!context.mounted) {
      logInfo(_tag, 'context not mounted, auto-selecting best quality');
      _autoSelectBest(msg);
      return;
    }

    _dialogOpen = true;
    showHlsQualityDialog(context, taskId: msg.taskId, options: msg.options);
    Future.microtask(() {
      _dialogOpen = false;
    });
  }

  void _autoSelectBest(HlsQualityOptions msg) {
    if (msg.options.isEmpty) return;
    int bestIdx = 0;
    int bestBw = 0;
    for (int i = 0; i < msg.options.length; i++) {
      if (msg.options[i].bandwidth > bestBw) {
        bestBw = msg.options[i].bandwidth.toInt();
        bestIdx = i;
      }
    }
    SelectHlsQuality(
      taskId: msg.taskId,
      selectedIndex: msg.options[bestIdx].index,
    ).sendSignalToRust();
  }
}
