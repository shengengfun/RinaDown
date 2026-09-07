---
title: Web UI
description: Sign in and manage downloads from the browser-based interface bundled with the headless server.
section: headless-server
order: 3
---

Once the server is running (see [Server Setup](/docs/en/headless-server/setup/)), open its address in a browser — for example `http://<host>:17800/` — to reach the Web UI. It mirrors the desktop app's task management workflow, adapted for remote, multi-session use.

## Signing in

The login screen asks for a **server address** (pre-filled with the current origin — leave it as-is unless you're pointing at a different host) and an **access token**. On first run — when the server has not set a key yet — the page becomes an initialization wizard: set and confirm an access key (you can also generate one at random). Once a key exists, you get the normal sign-in form. The token is the key you chose during initialization, or the one shown under **Settings → Security & Access** (see [Server Setup](/docs/en/headless-server/setup/)).

Checking **"Remember this device"** stores the token in `localStorage` (persists across browser restarts); leaving it unchecked stores it in `sessionStorage` (cleared when the tab closes). Nothing is ever sent anywhere except to the server itself.


<!-- TODO(screenshot): login screen with server address and token fields -->

## Main layout

The main screen is a three-pane layout, the same structure as the desktop app:

- **Sidebar** (left) — brand + global speed, file-type filters (All / Video / Audio / Documents / Images / Archives / Other) with live counts, queues, connection status badge, and sign-out. The queue list shows the built-in **Main Queue** and **Download Later** queues plus your named queues (create with the `+` button; delete a named queue from the hover trash icon — its tasks move back to the Main Queue). Each queue has a running/stopped state dot and hover buttons to **start/stop** it (stopping pauses every task in it and keeps them from being auto-started; starting resumes them in queue order) and to edit its **daily schedule** (start/stop times `HH:MM` — fill either or both — plus the weekdays it applies to).
- **Task list** (center) — top bar, batch management bar, status tabs, the virtualized task list, and a bottom status bar.
- **Detail panel** (right) — opens when you select a task; five tabs: **General**, **Segments**, **Queue**, **Log**, **Advanced**.

<!-- TODO(screenshot): full three-pane layout with a task selected and the detail panel open -->

### Top bar

- **Search** — filters the task list by file name. Press `Ctrl+F` (or `Cmd+F`) anywhere on the page to jump into the search box; `Esc` clears it.
- **Batch management toggle** — switches on the manage bar (checkboxes on every row).
- **Pause all / Resume all** — one button, its icon and action flip depending on whether any task is currently active.
- **Global speed limit** — shows the current limit at a glance; click it to jump to Settings to change it.
- **New download** — opens the new-download dialog (below).
- **Settings** — opens the settings screen.

### Status tabs and filtering

Status tabs (**All / Downloading / Completed / Paused / Error**) sit above the list, each with a live count. Combine them with the sidebar's file-type and queue filters and the search box — all four filters apply together.

### Batch management

With batch management on, every row gets a checkbox, and a bar appears with **Select all**, a running "N selected" count, and **Pause / Resume / Delete** buttons that act on the whole selection at once. Deleting from here does not delete files on disk (use the detail panel's per-task delete-with-files option for that). Click **Done** to exit batch mode.

### Task detail panel

Selecting a task opens the detail panel:

- **General** — progress, downloaded/total size, speed, thread (segment) count, download URL (with copy button), save path on the server, protocol and queue, creation time. For a **completed** task the primary action is **"Save to local"** (streams the file from the server to your browser via `/api/v1/tasks/{id}/file`); for any other task the primary action is **"Boost"** (pauses other tasks to free bandwidth for this one). Delete is always available and asks for confirmation.
- **Segments** — the same per-segment progress visualization as the desktop app, including the live split animation when the engine proactively splits a slow segment.
- **Queue** — move the task between named queues.
- **Log** — recent events for this task.
- **Advanced** — checksum and per-task proxy details.

Right-clicking a row in the task list opens a context menu with the same actions (Pause/Continue/Retry, Boost, Save to local, Copy download link, Copy server file path, Move to queue…, Delete, Delete and delete files) without opening the detail panel first.

<!-- TODO(screenshot): detail panel General tab for a completed task, showing the "Save to local" button -->

## Starting a new download

The **New download** dialog accepts one or more URLs (one per line — HTTP, FTP, magnet links, and M3U8 playlists are all accepted) and creates one task per line. Fields:

| Field | Notes |
|---|---|
| File name | Only editable for a single-URL submission; left blank it's inferred from the URL / `Content-Disposition`. |
| Threads | Segment count: Auto (`segment_advisor` picks based on file size) or a fixed 1/2/4/8/16/32. |
| Save directory | Defaults to the server's configured default save directory; browse the server's filesystem with the folder-picker button next to the field (this lists directories on the *server*, not your local machine). |
| Queue | Assign to a named queue or the Main Queue. |
| User-Agent | Global default or one of a few presets. |
| Advanced (collapsible) | Cookies, Referrer, a per-task proxy URL, and a Checksum (`algo=hexhash`, verified after the download completes). |

Two buttons submit the form: **Start Download** creates the task(s) and starts them right away, while **Download Later** creates them in a paused state parked in the built-in **Download Later** queue (start them individually or start that queue when you're ready). Submitting here creates tasks directly through the management API — there is no confirmation prompt, since you're already inside a trusted, authenticated session.

<!-- TODO(screenshot): new-download dialog with the advanced options panel expanded -->

### HLS quality and BitTorrent file selection

Some downloads need a decision mid-flight, delivered over the WebSocket connection:

- **HLS quality** — when a M3U8 playlist has multiple bitrate variants, a dialog lists each one (resolution and Mbps) with the highest-bandwidth option pre-selected. If you don't respond within 60 seconds, the server automatically picks the highest-bandwidth variant.
- **BitTorrent file selection** — for torrents/magnet links with multiple files, a dialog lists every file (with size and a running "N selected / total size" readout) so you can skip files you don't want; everything is selected by default.

These dialogs only appear while you have the Web UI open and connected — if you're not connected when the engine needs a decision, it falls back to its default (highest HLS bandwidth after the timeout; all BT files if nothing is chosen).

## Retrieving finished files

The server keeps completed files on its own filesystem. To pull one down to your local machine, use **Save to local** from the detail panel or the context menu — it streams the file through `GET /api/v1/tasks/{id}/file` as a normal browser download (with a `Content-Disposition: attachment` header and the original filename), authenticated via a token query parameter since browser-initiated downloads can't set custom headers.

## Settings

The settings screen (gear icon in the top bar) has seven categories:

| Category | Covers |
|---|---|
| General | Server-side behavior: max concurrent tasks, default thread count, auto-retry limit and delay. |
| Appearance | Theme mode (Light / Dark / Follow system) and accent color — stored locally in your browser, not on the server. |
| Download | Default save directory and other download-engine defaults, stored in the server's config table. |
| BitTorrent | `librqbit` engine parameters (DHT, trackers, etc.), server-side. |
| Proxy | Server-side outbound proxy. Unlike the desktop app, "System proxy" would require reading the *server's* registry/settings, so manual proxy configuration is the practical choice here. |
| Security & Access | Access token (view/copy/regenerate), and toggles for the aria2-compatible RPC (`/jsonrpc`) and script takeover (`/download`) entry points, plus the current WebSocket connection status. |
| About | Server version, update channel, the server-side **log directory** (with a copy button) and a one-click **Export logs as ZIP** download — handy for remote/NAS troubleshooting — plus sign-out. |

Each category mirrors a section of the desktop app's settings with the same underlying config keys; for the full list of fields and their meaning see [Settings](/docs/en/getting-started/settings/) in the desktop client docs.

Note that the Web UI itself has no language switcher — its interface text is fixed in Chinese regardless of the accent/theme you pick; only the desktop app has a language setting.

<!-- TODO(screenshot): Settings screen, Security & Access category, with the token box visible -->

## Connection status and reconnects

The sidebar's connection badge shows the WebSocket state: connecting, connected (with round-trip latency), or disconnected. If the connection drops — the server restarted, the network blipped — the client automatically retries with exponential backoff starting at 1 second and doubling up to a 15-second cap, until it reconnects or you sign out. Live progress, speed, and segment updates simply resume once the socket is back; you don't need to reload the page.
