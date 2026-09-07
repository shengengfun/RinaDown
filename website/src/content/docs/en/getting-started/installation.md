---
title: Installation
description: Install FluxDown on Windows, macOS, or Linux and get it running for the first time.
section: getting-started
order: 1
---

FluxDown ships full-featured native builds for Windows, macOS, and Linux. Every package contains the same Rust download engine and the same interface — there's no "lite" edition and no account to create.

## System Requirements

| Platform | Requirement |
|---|---|
| Windows | Windows 10 (64-bit) or later, x64 or ARM64 |
| macOS | macOS 10.15 (Catalina) or later, Apple Silicon or Intel |
| Linux | A 64-bit desktop distribution with a modern GTK3 stack |

## Windows

Grab a build from the [download page](/#download). Two options, both offered for x64 and ARM64:

- **Installer** — `FluxDown-<version>-setup.exe`. Runs the standard Inno Setup wizard and installs for the current user only (no admin rights required). During setup you can optionally check boxes to create a desktop shortcut, launch FluxDown at system startup, and associate `.torrent` files with FluxDown — all unchecked by default.
- **Portable** — `FluxDown-<version>-windows-<arch>-portable.zip`. Extract anywhere and run `flux_down.exe`. Nothing is written outside the extracted folder except whatever you opt into at first launch.

The build isn't code-signed, so Windows SmartScreen may flag it as coming from an "unknown publisher" the first time you run it. Click **More info → Run anyway** to continue.

### Scoop

If you use the [Scoop](https://scoop.sh) package manager, install FluxDown from the FluxDown bucket:

```powershell
scoop bucket add fluxdown https://github.com/zerx-lab/FluxDown
scoop install fluxdown/fluxdown
```

This installs the portable build and keeps your `settings.json` across upgrades. Update anytime with `scoop update fluxdown`.

## macOS

- **DMG** — `FluxDown-<version>-macos-<arch>.dmg` (`arm64` for Apple Silicon, `x64` for Intel). Open it and drag FluxDown into **Applications**.
- **Portable** — `FluxDown-<version>-macos-<arch>.tar.gz`. Extract it and run the app bundle directly.

The build isn't notarized, so Gatekeeper blocks the first launch with an "unidentified developer" warning. Right-click (or Control-click) the app and choose **Open**, or approve it under **System Settings → Privacy & Security → Open Anyway**.

## Linux

All Linux packages are x64:

- **AppImage** — `FluxDown-<version>-linux-x64.AppImage`. Make it executable (`chmod +x`) and run it. Distributions released in the last few years may need `libfuse2` installed for AppImages to launch.
- **deb** — `FluxDown-<version>-linux-x64.deb`, for Debian/Ubuntu and derivatives: `sudo apt install ./FluxDown-<version>-linux-x64.deb`.
- **Arch package** — `FluxDown-<version>-linux-x64.pkg.tar.zst`: `sudo pacman -U FluxDown-<version>-linux-x64.pkg.tar.zst`.
- **Portable** — `FluxDown-<version>-linux-x64.tar.gz`. Extract it and run the bundled binary.

## First Launch

FluxDown quietly wires itself into the OS the first time it starts, so links and files from other apps reach it without extra setup:

- **`fluxdown://` protocol** — registered automatically on every startup so the browser extension and other apps can hand off downloads to FluxDown.
- **Native Messaging Host** — registered automatically so the browser extension can talk to FluxDown over Native Messaging (a Windows Named Pipe or a Linux/macOS Unix socket).
- **`.torrent` file association** — *not* automatic. If you didn't check the installer's association box, FluxDown shows a one-time dialog on first launch asking whether to make it the default `.torrent` handler. Accept, dismiss, or change your mind later in **Settings → General → Associate .torrent Files**.

<!-- TODO(screenshot): 首次启动的 .torrent 文件关联提示对话框 -->

## Automatic Updates

Five seconds after startup, FluxDown silently checks GitHub Releases for a newer version in the background (this can be turned off in **Settings → About → Auto-check for Updates**). If an update is available, a dialog shows the changelog with **Update Now** and **Remind Me Later**. You can trigger a manual check any time from **Settings → About**, or from the version indicator at the bottom of the sidebar.

## Uninstalling

- **Windows (installer)** — open **Settings → Apps → Installed apps**, find FluxDown, and uninstall. For a portable install, just delete the extracted folder.
- **macOS** — drag FluxDown from **Applications** to the Trash.
- **Linux** — `sudo apt remove fluxdown` (deb), `sudo pacman -R fluxdown` (Arch), or delete the AppImage/extracted folder.

Uninstalling only removes the application itself — files you've already downloaded stay wherever you saved them.
