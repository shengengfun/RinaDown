---
title: Docker & NAS
description: Run the headless FluxDown server from the prebuilt Docker image, with Docker Compose, CasaOS/ZimaOS, Unraid, and native Synology DSM packages.
section: headless-server
order: 2
---

The fastest way to run the headless server is the prebuilt Docker image — no Cargo build, no separate Web UI build step. The image bundles the server binary and the Web UI, exposes everything on one port (`17800`), and persists its database, logs, and access key to a volume.

Image: `ghcr.io/zerx-lab/fluxdown-server` (tags: a specific version like `0.1.54`, or `latest`).

> Prefer a pinned version tag over `latest` for reproducible deployments.

## docker run

```bash
docker run -d \
  --name fluxdown-server \
  --restart unless-stopped \
  -p 17800:17800 \
  -v fluxdown-data:/data \
  -v /path/to/downloads:/root/Downloads \
  ghcr.io/zerx-lab/fluxdown-server:latest
```

- `/data` holds the database, logs, and the access key — keep it on a persistent volume.
- `/root/Downloads` is the container's default download directory (`HOME=/root`); bind it to a host path you want files written to.

On first visit to `http://<host>:17800/`, the Web UI opens an initialization wizard where you set the access key yourself (at least 8 characters, must include both letters and digits). Use that key to sign in to the Web UI and to authenticate the management API and MCP endpoint (`Authorization: Bearer <token>`).

For docker-compose or other orchestration, you can pre-set the key with `FLUXDOWN_TOKEN` and skip the wizard. It only takes effect when the instance has not set a key yet:

```bash
docker run -d \
  --name fluxdown-server \
  --restart unless-stopped \
  -p 17800:17800 \
  -e FLUXDOWN_TOKEN=your-secure-key-here \
  -v fluxdown-data:/data \
  -v /path/to/downloads:/root/Downloads \
  ghcr.io/zerx-lab/fluxdown-server:latest
```

## Docker Compose

```yaml
services:
  fluxdown-server:
    image: ghcr.io/zerx-lab/fluxdown-server:latest
    container_name: fluxdown-server
    restart: unless-stopped
    ports:
      - "17800:17800"
    volumes:
      - fluxdown-data:/data
      - ./downloads:/root/Downloads
    # environment:
    #   FLUXDOWN_TOKEN: your-secure-key-here   # optional: preset access key, skips the init wizard
    #   FLUXDOWN_LANG: zh
    #   FLUXDOWN_DATABASE_URL: postgres://user:pass@host:5432/fluxdown

volumes:
  fluxdown-data:
```

```bash
docker compose up -d
```

All environment variables from [Server Setup](/docs/en/headless-server/setup/) apply — most usefully `FLUXDOWN_LANG` (default Web UI language, `en`/`zh`) and `FLUXDOWN_DATABASE_URL` (point at an external PostgreSQL instead of the bundled SQLite).

## CasaOS / ZimaOS

FluxDown is published as a third-party CasaOS / ZimaOS app store, so you can install it with one click.

In CasaOS / ZimaOS: **App Store → Sources → Add**, then enter:

```
https://cdn.jsdelivr.net/gh/zerx-lab/casaos-appstore@gh-pages
```

Then install **FluxDown** from the store. Store source: [zerx-lab/casaos-appstore](https://github.com/zerx-lab/casaos-appstore).

## Unraid

An Unraid Community Applications template is available in [zerx-lab/unraid-templates](https://github.com/zerx-lab/unraid-templates). The Web UI is served at `http://[SERVER-IP]:17800/`.

## Synology NAS (native .spk package)

Every Server release ships native DSM packages — no Docker required. Four packages cover both DSM generations and both CPU families:

| Package | DSM version | CPU |
|---|---|---|
| `FluxDown-Server-<ver>-synology-dsm7-x64.spk` | DSM 7.0 or later | Intel / AMD (x86_64) |
| `FluxDown-Server-<ver>-synology-dsm7-arm64.spk` | DSM 7.0 or later | ARM64 (rtd1296, rtd1619b, armada37xx, …) |
| `FluxDown-Server-<ver>-synology-dsm6-x64.spk` | DSM 6.0 – 6.2 | Intel / AMD (x86_64) |
| `FluxDown-Server-<ver>-synology-dsm6-arm64.spk` | DSM 6.0 – 6.2 | ARM64 |

Not sure which CPU family your model uses? Check the "Package Arch" column for your model in [Synology's CPU list](https://kb.synology.com/en-us/DSM/tutorial/What_kind_of_CPU_does_my_NAS_have): `x86_64` family → x64 package, `armv8` family → arm64 package. Older `armv7`/`i686` models are not supported.

### Install

1. Open **Package Center → Settings → General** and set **Trust Level** to **Any publisher**. This is required because the packages are unsigned: DSM 7 removed third-party package signing entirely — the only "verified" packages are those distributed through Synology's official Package Center program.
2. **Package Center → Manual Install**, select the `.spk`, and follow the wizard.
3. Start the package, then click **Open** in Package Center — it links straight to the Web UI on port `17800` (`http://<NAS-IP>:17800`).

### First-run access key

On first open of the Web UI (`http://<NAS-IP>:17800`), the initialization wizard asks you to set an access key (at least 8 characters, letters and digits). That key is persisted in the package's database, so it survives restarts and upgrades. You can view or change it later under **Settings → Security & Access**.

### Permissions and data locations

- On **DSM 7** the service runs as a dedicated low-privilege package user (a DSM 7 platform requirement — packages may no longer run as root). On **DSM 6** it runs as root.
- Database, logs, and the access key live in `/var/packages/FluxDown/var`; downloads default to the same directory.
- To download into a shared folder on DSM 7, grant the package user write access first: **Control Panel → Shared Folder → Edit → Permissions**, switch the user dropdown to **System internal user**, and give **FluxDown** Read/Write. DSM 6 needs no grant (root).

### Upgrade and uninstall

Upgrade by manually installing a newer `.spk` over the existing one — the database, access key, and settings in `var` are preserved. Uninstalling from Package Center stops the service and removes the package.

## Exposing it safely

The image binds `0.0.0.0:17800` inside the container, mapped to the host. As with any headless deployment, the access key is the only thing guarding full remote control — see the [reverse proxy & TLS guidance](/docs/en/headless-server/setup/) before exposing it beyond a trusted LAN.

## Next steps

- [Web UI](/docs/en/headless-server/web-ui/) — sign in and manage downloads from a browser.
- [API Overview](/docs/en/api/overview/) — automate the server from scripts or other tools.
