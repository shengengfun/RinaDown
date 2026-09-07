// Repro for issue #100 (popup black frame): the popup engine renders its first
// frame while the window is ordered out, latching the view layer at the wrong
// contentsScale. On present (and on later backing changes) the host must
// re-assert the layer scale from the window's real scale AND force a
// backing-metrics re-push so the engine re-reads the pixel ratio.
// Drives the real PopupWindowHost via its method channels, compiled against
// the FlutterMacOS shim, with a real borderless NSWindow.

import Cocoa
import FlutterMacOS

@main
struct PopupPresentScaleTest {
    static var failures = 0

    static func check(_ ok: Bool, _ label: String) {
        print("\(ok ? "PASS" : "FAIL"): \(label)")
        if !ok { failures += 1 }
    }

    static func call(_ channel: FlutterMethodChannel, _ method: String, _ arguments: Any?) {
        guard let handler = channel.handler else {
            print("FATAL: no handler on channel \(channel.name)")
            exit(2)
        }
        handler(FlutterMethodCall(methodName: method, arguments: arguments)) { _ in }
    }

    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)

        PopupWindowHost.shared.register(with: ShimMessenger())
        guard let hostChannel = FlutterMethodChannel.registry["fluxdown/popup_host"] else {
            print("FATAL: fluxdown/popup_host not registered")
            exit(2)
        }

        // Cycle 1: create + present once so first-attach backing latching settles;
        // the regression is about re-presents where AppKit reports no change.
        call(hostChannel, "show", "{}")
        guard let childChannel = FlutterMethodChannel.registry["fluxdown/popup_child"] else {
            print("FATAL: fluxdown/popup_child not registered")
            exit(2)
        }
        call(childChannel, "reveal", ["height": 400.0])

        guard
            let win = app.windows.first(where: { $0.contentViewController is FlutterViewController }),
            let view = win.contentViewController?.view as? RecordingFlutterView
        else {
            print("FATAL: popup window/view not found")
            exit(2)
        }
        check(win.isVisible, "sanity: popup window is visible after reveal")

        call(hostChannel, "close", nil)

        // Cycle 2: latch a wrong scale while hidden (the ordered-out raster
        // state from the bug), then reveal — present must re-sync.
        call(hostChannel, "show", "{}")
        let landingScale = NSScreen.screens.first?.backingScaleFactor ?? win.backingScaleFactor
        let wrongScale: CGFloat = landingScale == 1.0 ? 2.0 : 1.0
        view.layer?.contentsScale = wrongScale
        view.resetMetricsPushes()

        call(childChannel, "reveal", ["height": 400.0])

        let truth = win.screen?.backingScaleFactor
            ?? NSScreen.screens.first?.backingScaleFactor
            ?? win.backingScaleFactor
        let presentedScale = view.layer?.contentsScale ?? -1
        check(presentedScale == truth,
              "present re-asserts layer.contentsScale to window scale (got \(presentedScale), want \(truth))")
        check(view.metricsPushes >= 1,
              "present forces a backing-metrics re-push (got \(view.metricsPushes) pushes)")

        // Backing change while the persistent window lives on (display move /
        // scale change) must re-sync too.
        view.layer?.contentsScale = wrongScale
        view.resetMetricsPushes()
        NotificationCenter.default.post(name: NSWindow.didChangeBackingPropertiesNotification, object: win)
        let notifiedScale = view.layer?.contentsScale ?? -1
        check(notifiedScale == truth,
              "backing-change notification re-asserts layer scale (got \(notifiedScale), want \(truth))")
        check(view.metricsPushes >= 1,
              "backing-change notification forces a metrics re-push (got \(view.metricsPushes) pushes)")

        call(hostChannel, "close", nil)
        exit(failures == 0 ? 0 : 1)
    }
}
