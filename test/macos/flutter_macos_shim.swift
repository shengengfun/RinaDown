// Test-only FlutterMacOS stand-in: just enough API surface for
// macos/Runner/PopupWindowHost.swift to compile under bare swiftc.

import Cocoa

public typealias FlutterResult = (Any?) -> Void

public let FlutterMethodNotImplemented = NSObject()

public protocol FlutterBinaryMessenger: AnyObject {}

public final class ShimMessenger: NSObject, FlutterBinaryMessenger {
    override public init() {}
}

public final class FlutterError: NSObject {
    public let code: String
    public let message: String?
    public let details: Any?
    public init(code: String, message: String?, details: Any?) {
        self.code = code
        self.message = message
        self.details = details
    }
}

public final class FlutterMethodCall: NSObject {
    public let method: String
    public let arguments: Any?
    public init(methodName: String, arguments: Any?) {
        self.method = methodName
        self.arguments = arguments
    }
}

public final class FlutterMethodChannel: NSObject {
    public private(set) static var registry: [String: FlutterMethodChannel] = [:]

    public let name: String
    public private(set) var handler: ((FlutterMethodCall, @escaping FlutterResult) -> Void)?
    public private(set) var invocations: [(method: String, arguments: Any?)] = []

    public init(name: String, binaryMessenger: FlutterBinaryMessenger) {
        self.name = name
        super.init()
        FlutterMethodChannel.registry[name] = self
    }

    public func setMethodCallHandler(_ handler: ((FlutterMethodCall, @escaping FlutterResult) -> Void)?) {
        self.handler = handler
    }

    public func invokeMethod(_ method: String, arguments: Any?) {
        invocations.append((method, arguments))
    }
}

public final class FlutterDartProject: NSObject {
    public var dartEntrypointArguments: [String]?
    override public init() {}
}

public final class FlutterEngine: NSObject {
    public let binaryMessenger: FlutterBinaryMessenger = ShimMessenger()
}

/// Stand-in FlutterView: records forced backing-metrics re-pushes. The real
/// FlutterView re-pushes window metrics to the engine (pixel ratio included)
/// from viewDidChangeBackingProperties.
public final class RecordingFlutterView: NSView {
    public private(set) var metricsPushes = 0
    public func resetMetricsPushes() { metricsPushes = 0 }
    override public func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        metricsPushes += 1
    }
}

public class FlutterViewController: NSViewController {
    public let engine = FlutterEngine()

    public init(project: FlutterDartProject) {
        super.init(nibName: nil, bundle: nil)
    }

    public required init?(coder: NSCoder) {
        return nil
    }

    override public func loadView() {
        view = RecordingFlutterView(frame: NSRect(x: 0, y: 0, width: 520, height: 600))
    }
}
