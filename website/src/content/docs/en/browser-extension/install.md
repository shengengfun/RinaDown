---
title: Install the Extension
description: Add the FluxDown browser extension for Chrome or Firefox and connect it to the desktop app.
section: browser-extension
order: 1
---

The FluxDown browser extension hooks into Chrome's or Firefox's download pipeline and hands matching downloads off to the FluxDown desktop app for high-speed, multi-threaded downloading. The extension can't download anything by itself — it needs the [desktop app](/docs/en/getting-started/installation/) installed, and started at least once. Install order doesn't matter: whichever piece you set up second will find the other automatically.

Chrome (Manifest V3) and Firefox are supported directly. The desktop app also registers itself for Microsoft Edge, and other Chromium-based browsers (Brave, Vivaldi, Opera, …) generally pick up the same registration Chrome uses, so loading the Chrome package manually (see below) works there too.

## Install from the Chrome Web Store

1. Open the [FluxDown extension page](https://chromewebstore.google.com/detail/fluxdown/meleenglfggcmcajknpeeeiobnpfmahc) on the Chrome Web Store.
2. Click **Add to Chrome**, then **Add extension** in the confirmation dialog.
3. Pin the extension (puzzle-piece icon in the toolbar → the pin next to FluxDown) so its status badge is always one click away.

<!-- TODO(screenshot): Chrome Web Store listing page for FluxDown with the "Add to Chrome" button highlighted -->

## Install from Firefox Add-ons

1. Open the [FluxDown add-on page](https://addons.mozilla.org/firefox/addon/fluxdown/) on addons.mozilla.org.
2. Click **Add to Firefox**, then **Add** in the permissions prompt.
3. Firefox 140 or later is required. If the install button is greyed out, update Firefox first.

## Manual install from a GitHub Release

Use this if you want a build newer than what's currently live on a store, or your organization only allows manually reviewed extensions.

**Chrome (and other Chromium browsers):**

1. Download `FluxDown-<version>-chrome.zip` from the [latest GitHub Release](https://github.com/zerx-lab/fluxdown/releases/latest) and unzip it somewhere permanent — Chrome loads the extension from that folder directly, not from the zip.
2. Open `chrome://extensions` and turn on **Developer mode** (top-right toggle).
3. Click **Load unpacked** and select the unzipped folder.
4. The unpacked build keeps the same extension identity as the Chrome Web Store build, so it connects to the desktop app exactly the same way — no extra setup needed.

**Firefox:**

Release builds ship as a signed `FluxDown-<version>-firefox.xpi`, which a regular (non-Developer-Edition) Firefox installs without extra flags:

1. Download `FluxDown-<version>-firefox.xpi` from the same Release.
2. Open `about:addons`, click the gear icon → **Install Add-on From File…**, and select the downloaded `.xpi`.

<!-- TODO(screenshot): chrome://extensions with Developer mode enabled and the Load unpacked button visible -->

## How the extension talks to the desktop app

The extension never opens a network connection to reach FluxDown. It uses the browser's Native Messaging mechanism: the browser launches a small relay process that the desktop app registered on your machine, and the extension exchanges messages with it over stdin/stdout. That relay forwards everything to the already-running desktop app over a local, OS-native channel — a Named Pipe on Windows, a Unix domain socket on Linux and macOS. The desktop app sets up this registration itself the first time it starts, which is why **it must be installed and opened at least once before the extension can do anything**.

## Confirm the connection

Click the FluxDown icon in the toolbar to open the popup. The badge in its top-right corner shows the live connection state:

| Badge | Meaning |
| --- | --- |
| Checking... (grey, pulsing) | The popup just opened and is pinging the desktop app. |
| Connected (green dot) | The desktop app answered — downloads will be handed off normally. |
| Disconnected (red dot) | The desktop app didn't answer in time — downloads fall back to the plain browser download until it does. |

Opening the popup only *checks* the connection; it doesn't start the app on its own — a plain connectivity check is intentionally not allowed to launch anything. If the badge reads Disconnected, either start FluxDown yourself, or click any download link: the extension launches the app automatically the moment it actually needs to hand off a file.

<!-- TODO(screenshot): extension popup header showing the Connected status badge -->

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| Badge stuck on Disconnected | Confirm the FluxDown desktop app has been installed and opened at least once — the extension can't reach an app that has never registered itself. |
| Badge only turns Connected after a download | Expected. The app auto-launches on the first real download request, not when you merely open the popup. Give it a few seconds — usually well under 10 — then reopen the popup to re-check. |
| Downloads keep falling back to the browser with a "FluxDown app not detected" notification | The app was unreachable when a download was attempted. Make sure FluxDown is running and try again; the extension stops falling back automatically as soon as it can reach the app, and won't repeat the notification for about 30 seconds either way. |
| Nothing changes after reinstalling or updating the desktop app | Restart the browser once. The desktop app registers its Native Messaging manifest on startup, and the browser only rereads that registration when it (re)launches. |
| Security software seems to block the connection | Native Messaging runs over a local pipe/socket rather than the network, so antivirus or sandboxing tools that intercept inter-process communication can still interfere. Allow FluxDown if you run this kind of software. |
| A specific site is never intercepted | Check whether its domain was added to **Excluded Domains** in the popup — see [Using the Extension](/docs/en/browser-extension/usage/). |
