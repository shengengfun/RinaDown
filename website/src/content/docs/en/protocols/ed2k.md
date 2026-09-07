---
title: eD2K
description: Download from the eDonkey2000 network via ed2k:// links, with server and Kad source finding and full hash verification.
section: protocols
order: 5
---

FluxDown includes a built-in eD2K (eDonkey2000) client — paste an `ed2k://` link into the New Download dialog and it downloads like any other task. The client is **download-only**: it fetches files from the network but does not share or upload, so no library management or upload quotas are involved.

## ed2k:// links

FluxDown accepts standard file links of the form:

```
ed2k://|file|<file name>|<size in bytes>|<eD2K hash>|/
```

The link itself carries the file name, exact size and content hash, so a new eD2K task starts with everything it needs — extra segments some sites append (such as `|h=...|` or `|s=...|`) are tolerated and ignored. File names using legacy Chinese encodings (GBK) are decoded correctly.

## Finding sources

FluxDown looks for peers that have the file through two complementary channels, both enabled by default:

- **eD2K servers** — the client queries a list of community servers. The list is the merge of two sources in *Settings → eD2K Servers*: the **Server List** you enter manually (one `host:port` per line) and a **Server Subscription** that periodically fetches community-maintained `server.met` lists (refreshed every 24 hours, with an *Update Now* button). Both are merged and de-duplicated automatically.
- **Kad DHT** — a decentralized lookup that works even when every server is down or unreachable. Toggle it with *Kad DHT source finding*.

If no sources are found, FluxDown retries a few times about a minute apart before marking the task as failed — rare files can take several attempts to locate.

<!-- TODO(screenshot): Settings → eD2K Servers page showing Server List, Server Subscription and Kad/UPnP toggles -->

## HighID and port mapping

Peers can only connect back to you if your eD2K port is reachable (called *HighID* in eD2K terms). FluxDown maps the port automatically through **UPnP** when your router supports it (*UPnP port mapping*, on by default). The *Listen port* setting picks the TCP/UDP port; the default `0` lets the system choose one automatically. With a LowID you can still download, but fewer peers will be able to serve you.

## Integrity verification

eD2K is built around content hashing, and FluxDown verifies at every level:

- Files are split into standard **9.28 MB parts**, and every part is hash-checked the moment it finishes downloading.
- A peer that delivers corrupt data is **blacklisted** for the rest of the task; a failing part is retried with other peers (up to 5 attempts per part).
- After the last part, the whole file is re-read from disk and verified against the hash in the link before being moved to its final name — a completed eD2K download is guaranteed to match the link bit-for-bit.

## Concurrency and resume

Several parts download in parallel, each from its own peer connection. The task's thread count setting controls this: the default is **4 parallel peers**, capped at 8. Progress is tracked per part in the local database, so pausing, quitting or a crash never loses verified parts — the task resumes exactly where it stopped.

## Related settings

| Setting | Location |
|---|---|
| Server List | Settings → eD2K Servers |
| Server Subscription | Settings → eD2K Servers |
| Kad DHT source finding | Settings → eD2K Servers |
| UPnP port mapping | Settings → eD2K Servers |
| Listen port | Settings → eD2K Servers |
| Default Threads (parallel peers) | Settings → Download |

## Limitations & FAQ

**Why is my eD2K download stuck at "no sources"?** The file may be rare or dead on the network. Make sure the server subscription has updated recently (*Settings → eD2K Servers* shows the server count and last update time) and Kad is enabled, then let the task retry — source discovery on eD2K can genuinely take minutes.

**Does FluxDown upload while downloading?** No. The client is strictly download-only; it never shares files back to the network.

**Speeds are lower than HTTP downloads of the same size.** That's inherent to peer-to-peer transfers: throughput depends on how many peers have the file and their upload capacity, not on your line speed alone. Obtaining HighID (see above) usually helps.
