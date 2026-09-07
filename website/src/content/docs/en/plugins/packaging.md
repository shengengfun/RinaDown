---
title: Packaging & Plugin Market
description: The .fxplug format, install safety limits, and publishing to the plugin index.
section: plugins
order: 5
---

## The `.fxplug` format

A `.fxplug` file is nothing more than a **zip of the plugin folder** with a different extension. Zip `manifest.json` and your `.js` files, rename to `my-plugin.fxplug`, done. Both layouts work:

```
my-plugin.fxplug          my-plugin.fxplug
├── manifest.json         └── my-plugin/          ← single wrapper folder
└── resolver.js               ├── manifest.json      is stripped automatically
                               └── resolver.js
```

`manifest.json` must be at the zip root or inside exactly one wrapper directory; anything else is rejected with "manifest.json not found".

### Install-time limits

Installers enforce these regardless of where the zip came from:

| Check | Limit |
|---|---|
| Zip entries | ≤ 200 |
| Total uncompressed size | ≤ 50 MB |
| Entry paths | Must stay inside the target folder — `..` and absolute paths are rejected (zip-slip protection) |
| Manifest | Parsed and fully validated (see [Manifest reference](/docs/en/plugins/manifest/)) |
| Scripts | Compile-checked; syntax errors abort the install |

The plugin is extracted to `<data dir>/plugins/<identity>/`. Installing the same identity again replaces the previous version.

## The plugin market

FluxDown's market is **a data format, not a service** — an index the app reads directly, with no backend and no account. The pieces:

- **An index** — a Git-versioned JSON file listing plugins, versions and download mirrors. The default index lives at `zerx-lab/fluxdown-plugin-index` on GitHub; anyone can fork it and run their own. Users can add custom index sources in the app.
- **Content addressing** — every published version records `contentHash = sha256(<the .fxplug file>)`. After downloading from any mirror, FluxDown recomputes the hash and rejects a mismatch. A compromised mirror cannot swap the payload.
- **Multiple mirrors** — each version lists several download URLs (raw.githubusercontent, jsDelivr, GitHub Releases…). FluxDown tries them in order. Mirror URLs must be `https` and must not point at private/loopback/metadata addresses.
- **Rollback protection** — each index carries a monotonically increasing `sequence`; the app remembers the highest value seen per index and refuses an older one.

v1 has no author-level cryptographic signatures — integrity rests on content addressing plus TLS plus Git history. The index schema reserves signature fields so they can be added later without breaking existing clients.

### Publishing a plugin

1. Build and test your plugin locally (dev mode, see [Your first plugin](/docs/en/plugins/your-first-plugin/)).
2. Zip it into a `.fxplug` and host the file somewhere permanently reachable over `https` — a GitHub Release is the usual choice.
3. Compute the hash: `sha256sum my-plugin.fxplug`.
4. Open a pull request against the index repository adding your plugin entry: identity, version, `contentHash`, mirror URLs, and a bumped `sequence`.

Once merged, the plugin appears in the in-app market on the next index refresh.

### Yanking a version

The index supports marking a version as yanked (for broken or malicious releases). Yanked versions are skipped by "install latest"; publish a fixed version with a higher version number the same way.

## Install APIs

Besides the settings page, plugins can be installed programmatically:

- `GET /api/v1/market` — fetch the merged market index.
- `POST /api/v1/market/install` with `{ "pluginId": "name@author" }` — install the latest non-yanked version.

Both are part of the management API group and require the management token — see the [API overview](/docs/en/api/overview/).
