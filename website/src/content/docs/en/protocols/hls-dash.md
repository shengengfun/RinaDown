---
title: HLS / DASH
description: Streaming video downloads — quality selection, AES-128 decryption and segment merging for M3U8 and MPD.
section: protocols
order: 3
---

HLS (HTTP Live Streaming, `.m3u8` playlists) and DASH (Dynamic Adaptive Streaming over HTTP, `.mpd` manifests) are the two formats most video sites use to serve streaming video: instead of one file, the video is split into many small segments listed in a playlist/manifest, often at several quality levels. FluxDown's engine reads the playlist, downloads every segment, and reassembles them into a single playable file — so you don't have to grab dozens or hundreds of tiny chunks yourself.

## Detecting streams

HLS/DASH downloads are most often started from the **FluxDown browser extension**, which sniffs network traffic per-tab and flags HLS/DASH manifests (including HLS's `application/vnd.apple.mpegurl` content type) as you browse or play a video, so you can grab them with one click. You can also paste a direct `.m3u8` or `.mpd` URL into the New Download dialog manually if you already have one.

## Quality selection

When a playlist offers more than one bitrate — a master playlist in HLS, multiple Representations in DASH — FluxDown shows a quality-selection dialog before downloading starts. Each option lists its bandwidth (e.g. "5.2 Mbps") and resolution, with friendly labels for common heights (4K, 2K, 1080p, 720p, 480p, 360p). Pick one and the download begins at that quality.

If the dialog can't be shown (for example, the app window isn't ready), FluxDown automatically picks the highest-bandwidth option rather than blocking the download.

<!-- TODO(screenshot): HLS quality-selection dialog listing multiple bandwidth/resolution options -->

## AES-128 decryption (HLS)

Many HLS streams encrypt their segments with AES-128-CBC, as specified by RFC 8216. FluxDown fetches the decryption key referenced by the playlist automatically — keys are cached so segments sharing the same key don't trigger repeated fetches — and decrypts every segment as it downloads. There's nothing to configure; encrypted and unencrypted playlists are handled the same way from your perspective.

For your privacy, cookies are only forwarded to segment and key requests that share the same origin as the playlist itself — they're never sent to a different, cross-origin CDN host even if one is referenced from the playlist.

## Segment download & merging

Segments download with bounded concurrency (by default up to 8 in parallel, capped at 16, tunable via *Settings → Download → Default Threads* — the same knob used for HTTP downloads) and are decrypted as they arrive, but always written to the output file strictly in playlist order, so the final file is identical to what a plain sequential download would produce. A segment that fails to download is retried up to 3 times with exponential backoff (2s, then 4s) before the whole task fails.

## DASH support

DASH support is more basic than HLS: FluxDown parses the MPD manifest and downloads the segments for the Representation you selected. DASH streams commonly split audio and video into separate tracks; when that happens, FluxDown automatically merges them into a single file using the system's `ffmpeg` (a fast stream copy — no re-encoding) if it's installed. If `ffmpeg` isn't available on your system, the audio and video are kept as two separate files (for example `video.mp4` and `video.audio.m4a`) that you'll need to merge yourself.

## Limitations & FAQ

**The video and audio downloaded as two separate files.** This happens when `ffmpeg` isn't installed on your system — FluxDown needs it to merge DASH's separate audio/video tracks into one file. Install `ffmpeg` and future DASH downloads will merge automatically; for files already downloaded, you can mux them yourself with `ffmpeg -i video.mp4 -i video.audio.m4a -c copy output.mp4`.

**A quality option I expected isn't listed.** FluxDown lists exactly what the master playlist or manifest advertises. If a site restricts higher qualities to a logged-in session, make sure the download was captured with your browser session's cookies (via the browser extension) rather than pasted in manually.

**Live streams.** FluxDown downloads what the playlist currently lists; it's built for downloading video-on-demand content rather than capturing an indefinitely growing live stream.
