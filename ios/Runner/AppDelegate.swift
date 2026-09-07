import Flutter
import UIKit
import flutter_foreground_task

@main
@objc class AppDelegate: FlutterAppDelegate {
  private var shareChannel: FlutterMethodChannel?
  /// 冷启动时暂存的分享 URL，等 Dart 侧首次 getInitialShare 取走。
  private var pendingShare: String?
  /// "打开文件"菜单存续期间必须强持有，否则菜单弹出即被释放。
  private var docController: UIDocumentInteractionController?

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    GeneratedPluginRegistrant.register(with: self)

    // flutter_foreground_task：后台 isolate 注册插件 + 前台通知展示
    SwiftFlutterForegroundTaskPlugin.setPluginRegistrantCallback { registry in
      GeneratedPluginRegistrant.register(with: registry)
    }
    if #available(iOS 10.0, *) {
      UNUserNotificationCenter.current().delegate = self as? UNUserNotificationCenterDelegate
    }

    if let controller = window?.rootViewController as? FlutterViewController {
      let channel = FlutterMethodChannel(
        name: "com.fluxdown/share",
        binaryMessenger: controller.binaryMessenger
      )
      channel.setMethodCallHandler { [weak self] call, result in
        if call.method == "getInitialShare" {
          result(self?.pendingShare)
          self?.pendingShare = nil
        } else {
          result(FlutterMethodNotImplemented)
        }
      }
      shareChannel = channel
    }

    // 与 Android MainActivity 同名通道：移动端"打开文件"
    if let controller = window?.rootViewController as? FlutterViewController {
      let storageChannel = FlutterMethodChannel(
        name: "com.fluxdown/storage",
        binaryMessenger: controller.binaryMessenger
      )
      storageChannel.setMethodCallHandler { [weak self] call, result in
        if call.method == "openFile" {
          self?.openFile(call: call, result: result)
        } else {
          result(FlutterMethodNotImplemented)
        }
      }
    }

    // 冷启动通过 URL scheme 打开时携带的链接
    if let url = launchOptions?[.url] as? URL {
      pendingShare = url.absoluteString
    }

    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  /// 应用运行中经 URL scheme（fluxdown:// / magnet:）唤起。
  override func application(
    _ app: UIApplication,
    open url: URL,
    options: [UIApplication.OpenURLOptionsKey: Any] = [:]
  ) -> Bool {
    let shared = url.absoluteString
    if let channel = shareChannel {
      channel.invokeMethod("onShare", arguments: shared)
    } else {
      pendingShare = shared
    }
    return true
  }

  /// 经 UIDocumentInteractionController 弹出"打开方式"菜单（沙箱内文件
  /// 无法直接交给其他 app，必须走系统分享/打开菜单让用户选择）。
  /// 错误码与 Android 侧一致：bad_args / not_found / no_handler。
  private func openFile(call: FlutterMethodCall, result: @escaping FlutterResult) {
    guard let args = call.arguments as? [String: Any],
          let path = args["path"] as? String, !path.isEmpty else {
      result(FlutterError(code: "bad_args", message: "path is required", details: nil))
      return
    }
    guard FileManager.default.fileExists(atPath: path) else {
      result(FlutterError(code: "not_found", message: "file not found: \(path)", details: nil))
      return
    }
    guard let rootView = window?.rootViewController?.view else {
      result(FlutterError(code: "no_handler", message: "no root view", details: nil))
      return
    }
    let dc = UIDocumentInteractionController(url: URL(fileURLWithPath: path))
    docController = dc
    let presented = dc.presentOptionsMenu(from: rootView.bounds, in: rootView, animated: true)
    if presented {
      result(true)
    } else {
      docController = nil
      result(FlutterError(code: "no_handler", message: "no app can open this file", details: nil))
    }
  }
}
