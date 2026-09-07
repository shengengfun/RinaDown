import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import '../../i18n/locale_provider.dart';
import '../../services/log_service.dart';
import '../mobile_ui.dart';

const _tag = 'MobileStorage';

/// Android 存储桥（对应 MainActivity.kt 的 `com.fluxdown/storage` channel）。
///
/// - 目录选择走系统文件管理器（SAF），Kotlin 侧将 tree URI 映射为
///   Rust 引擎可 std::fs 直写的文件系统路径；
/// - 公共目录（如 /storage/emulated/0/Download)写入需「所有文件访问」
///   权限（API 30+），由 [pickMobileDownloadDirectory] 统一引导。
class MobileStorageService {
  MobileStorageService._();

  static const _channel = MethodChannel('com.fluxdown/storage');

  /// 当前平台是否支持系统目录选择器
  static bool get supported => Platform.isAndroid;

  /// 是否为应用专属路径（免权限可写）
  static bool isAppPrivatePath(String path) =>
      path.contains('/Android/data/') || path.startsWith('/data/');

  /// 调起 SAF 目录选择器。
  /// 返回：文件系统路径；`null` = 用户取消；`''` = 无法映射为路径。
  static Future<String?> pickDirectoryRaw() =>
      _channel.invokeMethod<String>('pickDirectory');

  /// 是否已具备写公共目录的权限
  static Future<bool> hasAllFilesAccess() async =>
      await _channel.invokeMethod<bool>('hasAllFilesAccess') ?? false;

  /// 引导授权（API 30+ 跳系统设置；API <30 运行时权限弹窗）
  static Future<void> requestAllFilesAccess() =>
      _channel.invokeMethod<void>('requestAllFilesAccess');

  /// 应用专属外部下载目录（`Android/data/<pkg>/files/Download`）。
  ///
  /// 调用本身会让 framework 创建该目录树——Android/data 层禁止应用
  /// 自建子树，Rust 引擎 std::fs 直写前必须先经此初始化。
  static Future<String?> appExternalDownloadDir() async {
    if (!supported) return null;
    try {
      return await _channel.invokeMethod<String>('getExternalDownloadDir');
    } on PlatformException catch (e) {
      logInfo(_tag, 'getExternalDownloadDir failed: ${e.message}');
      return null;
    }
  }
}

/// 调起系统文件管理器选择下载目录，并处理：
/// - URI 无法映射 → Toast 提示重选；
/// - 选择了公共目录但缺少「所有文件访问」权限 → 弹窗引导去系统设置授权。
///
/// 返回选中的文件系统路径；用户取消或失败返回 `null`。
Future<String?> pickMobileDownloadDirectory(BuildContext context) async {
  final s = LocaleScope.of(context);

  String? path;
  try {
    path = await MobileStorageService.pickDirectoryRaw();
  } on PlatformException catch (e) {
    logInfo(_tag, 'pickDirectory failed: ${e.code} ${e.message}');
    path = '';
  }

  if (path == null) return null; // 用户取消
  if (!context.mounted) return null;
  if (path.isEmpty) {
    showMobileToast(context, s.mobilePickDirUnmappable);
    return null;
  }

  // 公共目录需要「所有文件访问」权限，缺失时引导授权
  if (!MobileStorageService.isAppPrivatePath(path)) {
    bool granted = false;
    try {
      granted = await MobileStorageService.hasAllFilesAccess();
    } on PlatformException catch (e) {
      logInfo(_tag, 'hasAllFilesAccess failed: ${e.message}');
    }
    if (!granted && context.mounted) {
      await showMobileConfirm(
        context,
        title: s.mobileAllFilesTitle,
        message: s.mobileAllFilesDesc,
        confirmLabel: s.mobileGoGrant,
        cancelLabel: s.cancel,
      ).then((ok) {
        if (ok == true) MobileStorageService.requestAllFilesAccess();
      });
    }
  }
  return path;
}
