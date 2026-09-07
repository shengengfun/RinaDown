// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "tray_manager",
    platforms: [
        .macOS("10.14")
    ],
    products: [
        .library(name: "tray-manager", targets: ["tray_manager"])
    ],
    dependencies: [],
    targets: [
        .target(
            name: "tray_manager",
            dependencies: [],
            path: "Classes"
        )
    ]
)
