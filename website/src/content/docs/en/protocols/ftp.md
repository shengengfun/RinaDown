---
title: FTP
description: Multi-connection FTP downloads with REST resume, embedded credentials and SOCKS/HTTP proxy support.
section: protocols
order: 4
---

FluxDown downloads from FTP servers with its own dedicated engine — no separate FTP client needed. Like HTTP downloads, larger files are split across multiple independent connections, and downloads resume from the exact byte offset after a pause or interruption.

## Multi-connection acceleration

Each segment of an FTP download opens its **own independent connection** to the server — the standard approach for parallel FTP. FluxDown caps this at **4 simultaneous connections** per download, regardless of file size or thread settings, because most FTP servers only allow 5–10 connections per client IP and going higher risks connection refusals or an IP ban. Files of 1 MB or smaller always download on a single connection, since splitting them wouldn't help.

Before enabling multiple connections, FluxDown runs a lightweight, content-independent probe to confirm the server actually honours the `REST` (resume) command it's about to rely on. Some older or proxied FTP servers accept `REST` but then ignore it and stream the file from byte 0 regardless — using multiple connections against a server like that would silently scramble the downloaded file. When FluxDown detects this, it automatically falls back to a single connection instead.

## Resume support

FTP resume works through the `REST` command, which tells the server to start sending from a specific byte offset — used both for single- and multi-connection downloads. Progress is written to FluxDown's local database periodically, so pausing the task, closing the app, or a crash loses only a few seconds of progress rather than the whole file. On resume, a single-connection download picks up from the length of the partial file already on disk; a multi-connection download resumes each segment independently from where it left off.

## Credentials in the URL

FTP downloads don't have a separate username/password field — embed the credentials directly in the URL:

```
ftp://user:pass@host/path/to/file.iso
```

Leave off the `user:pass@` part entirely for an anonymous login (FluxDown defaults to the standard `anonymous` / `anonymous@` credentials). If your password itself contains an `@`, percent-encode it as `%40` so it isn't mistaken for the host separator.

## Proxy support

FTP downloads support **SOCKS4, SOCKS5, HTTP and HTTPS** proxies, tunneling both the control connection and the data connections through the same proxy. Configure a global proxy in *Settings → Proxy*, or override it per task in the New Download dialog's advanced options using the same format as HTTP downloads:

```
http://host:port
socks5://host:port
http://user:pass@host:port
socks5://user:pass@host:port
```

Leave the per-task field empty to use the global proxy configuration.

<!-- TODO(screenshot): New Download dialog advanced options showing the Task Proxy field for an ftp:// URL -->

## Related settings

| Setting | Location |
|---|---|
| Default Threads | Settings → Download |
| Max Concurrent Downloads | Settings → Download |
| Speed Limit | Settings → Download |
| Proxy (None / System / Manual, HTTP/HTTPS/SOCKS4/SOCKS5) | Settings → Proxy |

## Limitations & FAQ

**My FTP download only uses one connection even though I picked more threads.** Either the file is 1 MB or smaller, or FluxDown's REST-honouring probe determined the server doesn't correctly support resuming multi-segment transfers — both fall back to a single connection to guarantee a correct file over raw speed.

**Why cap FTP at 4 connections when HTTP can use far more?** FTP servers are typically far stricter about per-IP connection limits than HTTP/CDN servers; 4 is a conservative ceiling chosen to avoid connection refusals or bans on shared hosting.

**Anonymous FTP isn't working.** Double-check the URL has no stray `user:pass@` left in it — if it's present but empty or malformed, FluxDown won't fall back to anonymous login automatically.
