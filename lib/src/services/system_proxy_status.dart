// 系统代理检测状态服务 —— 依赖 bindings(rinf 信号),仅主引擎侧可 import。
// popup 独立窗进程禁止 import 本文件(其数据只能走 QuickPopupPayload 注入)。

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:rinf/rinf.dart';

import '../bindings/bindings.dart';
import '../models/settings_provider.dart';

/// 全局单例:发送 DetectSystemProxy 信号并缓存最近一次 SystemProxyInfo 结果。
class SystemProxyStatusService extends ChangeNotifier {
  SystemProxyStatusService._() {
    _sub = SystemProxyInfo.rustSignalStream.listen(_onResult);
  }

  static final instance = SystemProxyStatusService._();

  // 订阅生命周期与应用一致,无需取消
  // ignore: unused_field
  late final StreamSubscription<RustSignalPack<SystemProxyInfo>> _sub;

  bool _detecting = false;
  bool _detected = false;
  String _summary = '';

  /// 检测请求在途
  bool get detecting => _detecting;

  /// 最近一次检测是否发现系统代理
  bool get detected => _detected;

  /// 检测到的代理摘要,如 'http://127.0.0.1:7890';未检测到为 ''
  String get summary => _summary;

  /// 发送 DetectSystemProxy 信号(在途去重)。
  ///
  /// 信号发送失败(单元测试无 Rust 动态库等)静默降级为「未检测」——
  /// 检测是纯增强,绝不允许它打断调用方的 UI 流程(如 popup show)。
  void refresh() {
    if (_detecting) return;
    _detecting = true;
    notifyListeners();
    try {
      DetectSystemProxy().sendSignalToRust();
    } catch (_) {
      _detecting = false;
      notifyListeners();
    }
  }

  void _onResult(RustSignalPack<SystemProxyInfo> pack) {
    final msg = pack.message;
    _detecting = false;
    _detected = msg.detected;
    _summary = msg.detected && msg.host.isNotEmpty
        ? '${msg.proxyType.isEmpty ? 'http' : msg.proxyType}://${msg.host}'
              '${msg.port.isEmpty ? '' : ':${msg.port}'}'
        : '';
    notifyListeners();
  }
}

/// 从全局设置拼出手动代理 URL:`type://[user:pass@]host:port`。
///
/// userinfo 使用百分号编码;host 为空或 port 解析为 0/非法时返回 null。
String? manualProxyUrlFromSettings(SettingsProvider sp) {
  final host = sp.proxyHost.trim();
  final port = int.tryParse(sp.proxyPort.trim()) ?? 0;
  if (host.isEmpty || port == 0) return null;
  final type = sp.proxyType.trim().isEmpty ? 'http' : sp.proxyType.trim();
  final user = sp.proxyUsername;
  final pass = sp.proxyPassword;
  var userinfo = '';
  if (user.isNotEmpty) {
    userinfo = Uri.encodeComponent(user);
    if (pass.isNotEmpty) {
      userinfo += ':${Uri.encodeComponent(pass)}';
    }
    userinfo += '@';
  }
  return '$type://$userinfo$host:$port';
}
