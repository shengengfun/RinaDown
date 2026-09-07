#!/bin/sh
# Repro/regression test for issue #100 (popup black frame): compiles the real
# macos/Runner/PopupWindowHost.swift against a minimal FlutterMacOS shim and
# drives the show/reveal handshake with a real NSWindow. No xcodebuild needed.
set -eu
cd "$(dirname "$0")/.."

BUILD_DIR="$(mktemp -d)"
trap 'rm -rf "$BUILD_DIR"' EXIT

swiftc -c test/macos/flutter_macos_shim.swift \
    -parse-as-library -module-name FlutterMacOS \
    -emit-module -emit-module-path "$BUILD_DIR/FlutterMacOS.swiftmodule" \
    -o "$BUILD_DIR/FlutterMacOS.o"

swiftc -I "$BUILD_DIR" \
    test/macos/popup_present_scale_test.swift \
    macos/Runner/PopupWindowHost.swift \
    "$BUILD_DIR/FlutterMacOS.o" \
    -o "$BUILD_DIR/popup_present_scale_test"

"$BUILD_DIR/popup_present_scale_test"
