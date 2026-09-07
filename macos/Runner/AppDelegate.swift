import Cocoa
import FlutterMacOS
import UserNotifications

@main
class AppDelegate: FlutterAppDelegate, UNUserNotificationCenterDelegate {
  override func applicationDidFinishLaunching(_ notification: Notification) {
    UNUserNotificationCenter.current().delegate = self
    // 不能调 super：FlutterAppDelegate 只实现了 applicationWillFinishLaunching:，
    // 没有 applicationDidFinishLaunching:（见 Flutter engine
    // shell/platform/darwin/macos/framework/Source/FlutterAppDelegate.mm）。
    // Swift 允许写 super 是因为该方法是 NSApplicationDelegate 的 optional 协议
    // 要求，运行时 objc_msgSendSuper 落空 → NSInvalidArgumentException：
    //   -[FluxDown.AppDelegate applicationDidFinishLaunching:]: unrecognized selector
    // 异常会从 -[NSApplication _postDidFinishNotification] 一路抛穿
    // _handleAEOpenEvent:，使排在本 delegate 之后注册的
    // NSApplicationDidFinishLaunchingNotification 观察者全部收不到通知，
    // 且被 AppKit 在 runloop 边界吞掉（不崩溃、不产生崩溃报告）。
  }

  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    return false
  }

  /// 点击 Dock 图标时恢复主窗口（详见 restoreMainWindow）。
  override func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
    restoreMainWindow()
    return false
  }

  /// 隐藏主窗口并切换为仅菜单栏驻留的 accessory 应用。
  ///
  /// 必须先 orderOut 主窗口再隐藏 Dock，避免可见窗口短暂处于没有应用菜单的
  /// accessory 状态。返回 false 时 Dart 侧会走 window_manager 降级路径。
  @discardableResult
  func hideMainWindowToTray() -> Bool {
    guard let window = mainFlutterWindow else { return false }
    window.orderOut(nil)
    return NSApp.setActivationPolicy(.accessory)
  }

  /// 可靠地把主窗口从「关闭到托盘」(orderOut) 或最小化恢复并置于前台。
  ///
  /// 供 Dock 点击 (applicationShouldHandleReopen) 与托盘/悬浮球点击
  /// (MethodChannel `com.fluxdown/window` → restore) 共用。
  ///
  /// 先恢复 regular 策略，确保 Dock 与应用菜单在窗口出现前完成恢复。
  /// 不走 window_manager 的 show()/focus()：其 focus() 使用
  /// NSApp.activate(ignoringOtherApps: false)，在用户已切到别的 App 后
  /// 点击托盘时，macOS 13+ 常常不把本 App 带到前台，导致窗口 orderFront
  /// 后仍停留在后台不可见，用户以为「打不开」只能退出重开。这里统一用
  /// ignoringOtherApps: true 强制前台。
  /// 注意：不遍历 NSApp.windows —— 悬浮球 FloatingBallPanel 也在其中，
  /// 不能被激活聚焦。
  @discardableResult
  func restoreMainWindow() -> Bool {
    let policyRestored = NSApp.setActivationPolicy(.regular)
    guard let window = mainFlutterWindow else { return false }
    if window.isMiniaturized {
      window.deminiaturize(nil)
    }
    if !window.isVisible {
      window.setIsVisible(true)
    }
    window.makeKeyAndOrderFront(self)
    NSApp.activate(ignoringOtherApps: true)
    return policyRestored
  }

  override func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
    return true
  }

  // MARK: - fluxdown:// URL scheme

  /// 冷启动时暂存的协议 URL：URL open 事件可能早于 Dart 侧
  /// `com.fluxdown/single_instance` 的 handler 注册，先缓冲再重试投递。
  private var pendingUrls: [String] = []
  private var deliverRetryCount = 0

  /// 系统派发 `fluxdown://` URL（CFBundleURLTypes 注册）：与 Windows 的
  /// WM_COPYDATA / Linux 的 GApplication::open 同构 —— 把 URL 字符串按
  /// `onSecondInstance` 参数格式转发给 Dart（main.dart 复用同一条
  /// 启动参数解析链），并把主窗口带到前台。
  override func application(_ application: NSApplication, open urls: [URL]) {
    let args = urls.map { $0.absoluteString }.filter { !$0.isEmpty }
    guard !args.isEmpty else { return }
    pendingUrls.append(contentsOf: args)
    deliverRetryCount = 0
    deliverPendingUrls()
    restoreMainWindow()
  }

  /// 投递缓冲的 URL；Dart handler 尚未注册（冷启动竞态）时按 0.5s 间隔
  /// 重试，最多 20 次（10s 覆盖首帧 + initState 的最坏情况）。
  private func deliverPendingUrls() {
    guard !pendingUrls.isEmpty else { return }
    guard
      let controller = mainFlutterWindow?.contentViewController
        as? FlutterViewController
    else {
      scheduleDeliverRetry()
      return
    }
    let channel = FlutterMethodChannel(
      name: "com.fluxdown/single_instance",
      binaryMessenger: controller.engine.binaryMessenger
    )
    let args = pendingUrls
    channel.invokeMethod("onSecondInstance", arguments: args) { [weak self] result in
      guard let self else { return }
      let notImplemented =
        (result as? NSObject) === FlutterMethodNotImplemented
      if result is FlutterError || notImplemented {
        // handler 未注册（冷启动竞态）或出错：保留缓冲重试
        self.scheduleDeliverRetry()
      } else {
        // 成功（Dart handler 返回 null → nil/NSNull，同样算成功）
        self.pendingUrls.removeAll()
      }
    }
  }

  private func scheduleDeliverRetry() {
    guard deliverRetryCount < 20 else {
      pendingUrls.removeAll()
      return
    }
    deliverRetryCount += 1
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
      self?.deliverPendingUrls()
    }
  }
}
