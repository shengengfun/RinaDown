---
title: Server Setup
description: Build and run the headless FluxDown server, configure it with environment variables, and expose it safely.
section: headless-server
order: 1
---

`fluxdown_server` is a headless build of the FluxDown download engine: no Flutter UI, no Rinf/FFI layer. It exposes the same Rust engine (HTTP/HTTPS, FTP, BitTorrent, HLS, DASH) over HTTP, WebSocket, and a Web UI baked into the executable, so you can run it on a NAS, a home server, or a VPS and manage downloads remotely from a browser. Releases ship a **single self-contained binary** — no companion `web/` folder to copy around.

For most deployments the prebuilt Docker image is the easiest path — see [Docker & NAS](/docs/en/headless-server/docker/). This page covers building and running from the workspace source with Cargo, plus configuration that applies to both.

## Build and run

The server lives in the `native/server` crate (package name `fluxdown_server`, binary name `fluxdown-server`). From the repository root:

```bash
# Development run (debug build, default bind 0.0.0.0:17800)
cargo run -p fluxdown_server

# Production build
cargo build --release -p fluxdown_server
# binary at target/release/fluxdown-server (fluxdown-server.exe on Windows)
```

The process is self-contained: it opens its own SQLite (or PostgreSQL) database, runs the download engine, and serves the Web UI from bytes embedded in the executable — no separate database server, static file directory, or reverse proxy is required to get started.

## Building the web front end

Only needed when you build the server yourself — official release binaries and Docker images already contain the UI.

The Web UI is a separate SPA in `web/` (React 19 + TanStack, built with [Bun](https://bun.sh)). Its build output is embedded into the server binary **at compile time**, so it must exist *before* you build the server:

```bash
cd web
bun install
bun run build      # outputs to web/dist

cd ..
cargo build --release -p fluxdown_server   # embeds web/dist into the binary
```

Rebuild the server after every front-end change — the running binary keeps serving the bytes it was compiled with. Two compile-time knobs:

- `FLUXDOWN_EMBED_WEBROOT` — embed a different directory instead of `web/dist` (used by CI, which builds the SPA in a separate job).
- If the directory is missing or empty, the build still succeeds with a warning; the server then answers browser requests with a `503` page explaining how to fix it, while the REST API and WebSocket keep working normally.

<!-- TODO(screenshot): browser showing the first-run “Initialize FluxDown Server” wizard -->

## Environment variables

All configuration is read once at startup from environment variables. There is no config file.

| Variable | Default | Description |
|---|---|---|
| `FLUXDOWN_BIND` | `0.0.0.0:17800` | TCP address the HTTP/WebSocket server listens on. |
| `FLUXDOWN_DATA_DIR` | Platform auto-detected (see below) | Directory holding the database file and logs. |
| `FLUXDOWN_DATABASE_URL` | unset — uses a SQLite file inside the data dir | Explicit connection string: `sqlite:/path/to/file.db` or `postgres://user:pass@host/db`. |
| `FLUXDOWN_WEBROOT` | unset — serves the embedded Web UI | Optional override: serve the SPA from this directory instead of the embedded copy (custom front end, or a hot-swapped `bun run build` output). There is **no** implicit `./web` lookup next to the executable. |
| `FLUXDOWN_TOKEN` | unset — first-run Web setup wizard | Optional pre-set management access key. Applied only when the database has no key yet (value is trimmed; must satisfy the key rules below, otherwise ignored with a warning). Use for unattended docker-compose / k8s / CI deploys that skip the wizard. |
| `FLUXDOWN_DEMO` | unset (off) | Truthy value (`1`/`true`/`yes`/`on`) turns on demo mode: only a built-in, generated 64 MiB file can be downloaded. Useful for public demos. |
| `FLUXDOWN_DEMO_URL` | unset (off) | Overrides demo mode's allowed URL with a specific one instead of the built-in generated file. |
| `FLUXDOWN_LANG` | unset (falls back to browser language) | Default Web UI language (`en`/`zh`; regional variants like `zh-CN` accepted). Pure fallback: once any user saves a language in Settings, the saved value becomes the server-side default (applies live, survives restarts); users who explicitly picked a language in their browser always keep their own choice. |

When `FLUXDOWN_DATA_DIR` is not set, the data directory is auto-detected the same way the desktop app does:

| Platform | Directory |
|---|---|
| Windows (portable build) | next to the executable |
| Windows (installed) | `%LOCALAPPDATA%\FluxDown\` |
| Linux | `$XDG_DATA_HOME/fluxdown/` |
| macOS | `~/Library/Application Support/fluxdown/` |

For a headless deployment you almost always want to set `FLUXDOWN_DATA_DIR` explicitly to a stable, backed-up path instead of relying on auto-detection.

```bash
FLUXDOWN_BIND=0.0.0.0:8080 \
FLUXDOWN_DATA_DIR=/srv/fluxdown/data \
./fluxdown-server
```

## First run: set the access key in the Web UI

The management API is always enabled on the headless server (unlike the desktop app, where it is opt-in). On first boot, if no access key is stored yet, the server enters a **pending setup** state: every management endpoint (`/api/v1/*`, `/mcp`) returns 403, while the Web SPA remains reachable so you can finish initialization in the browser.

stderr prints a bilingual guidance banner (not a generated secret):

```
==============================================================
  FluxDown Server: first run — no access key is set yet.
  Open the Web UI and create one:
    http://<server-ip>:17800/
  Requirements: 8+ characters, letters and digits.
  Unattended deploys can preset it via FLUXDOWN_TOKEN.
  ---
  首次运行：尚未设置访问密钥。请打开上面的 Web 界面自行设置
  （至少 8 位，必须同时包含字母和数字）。
==============================================================
```

Open that URL. The login page becomes an **Initialize FluxDown Server** wizard (not a normal sign-in form): enter an access key, confirm it, optionally click the button to random-generate one (`fxd_` + 24 characters), optionally check “Remember this device”, then save. You are signed into the main UI immediately — no server restart.

Key rules (enforced the same way in the Web UI and the API):

- ASCII printable characters only (no spaces, no non-ASCII)
- Length 8–128
- Must contain both letters and digits

Once saved, the key lives in the `config` table of the server's own database and survives restarts as long as the database file (or PostgreSQL database) persists. Use it to:

- Sign in to the Web UI (see [Web UI](/docs/en/headless-server/web-ui/)).
- Authenticate management API calls with `Authorization: Bearer <token>` (see [API Overview](/docs/en/api/overview/)).

This flow replaced the old “generate a token and print it once to stderr” approach because NAS users (Synology, QNAP, Unraid, and similar) often cannot see container or package stderr — a one-shot printed secret effectively locked them out.

### Unattended deploys

To skip the wizard (docker-compose, Kubernetes, CI), preset the key with `FLUXDOWN_TOKEN`. It is adopted only when the database still has no key:

```bash
FLUXDOWN_TOKEN='your-strong-key-here' ./fluxdown-server
```

### Security note

The setup window is first-come, first-served: whoever reaches the wizard first sets the key. Finish initialization (or preset `FLUXDOWN_TOKEN`) before exposing the server on an untrusted network.

### Resetting the access key

If you lose the key or suspect it leaked, change it from the Web UI (**Settings → Security & Access**) or by calling the management API while authenticated with the current key:

```bash
curl -X POST http://<host>:17800/api/v1/token/regenerate \
  -H "Authorization: Bearer <current-token>"
```

The new key takes effect **immediately** — the old one is invalidated at the same moment. No server restart is required. On the headless server the access key cannot be cleared: writing `local_server_token: ""` via `PUT /api/v1/config` returns 400.

## Database: SQLite and PostgreSQL

By default the server opens a SQLite database file inside the data directory — no setup needed. For multi-instance or higher-throughput deployments, point it at PostgreSQL instead:

```bash
FLUXDOWN_DATABASE_URL=postgres://fluxdown:password@localhost/fluxdown \
cargo run -p fluxdown_server
```

The connection string's scheme (`sqlite:` vs `postgres:`) selects the backend; both share the same schema and migrations. Credentials in `FLUXDOWN_DATABASE_URL` are masked in the server's own log output, but treat the environment variable itself like any other secret (avoid putting it in shell history or committing it to a process manager's config in plaintext where avoidable).

## Exposing it safely (reverse proxy & TLS)

`FLUXDOWN_BIND` defaults to `0.0.0.0:17800` — reachable on every network interface, unlike the desktop app's local API which is hardcoded to loopback only. That is intentional for headless use, but it means **you** are responsible for the network boundary:

- The management access key is the only thing standing between the internet and full remote control of your server (create/delete downloads, browse the server's filesystem via the directory picker, stream any completed file back). Treat it like a root password: don't share it, don't log it, rotate it if it may have leaked.
- If the server is reachable beyond a trusted LAN, put it behind a reverse proxy (nginx, Caddy, Traefik) terminating TLS, and only expose HTTPS. The Web UI's login screen sends the key in a request body/query string; on plain HTTP that is visible to anyone on the network path.
- The WebSocket endpoint (`/api/v1/ws`) needs `Upgrade`/`Connection` headers forwarded by the proxy. A minimal nginx snippet:

  ```nginx
  location / {
      proxy_pass http://127.0.0.1:17800;
      proxy_http_version 1.1;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "upgrade";
      proxy_set_header Host $host;
  }
  ```

- Prefer binding to a private interface (`FLUXDOWN_BIND=127.0.0.1:17800` and letting the reverse proxy sit in front, or a VPN/Tailscale address) over exposing the port directly to the public internet, even with TLS.

## Running as a systemd service

A minimal unit file for a Linux deployment (adjust paths and user):

```ini
[Unit]
Description=FluxDown headless download server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=fluxdown
Group=fluxdown
WorkingDirectory=/opt/fluxdown
Environment=FLUXDOWN_BIND=0.0.0.0:17800
Environment=FLUXDOWN_DATA_DIR=/var/lib/fluxdown
ExecStart=/opt/fluxdown/fluxdown-server
Restart=on-failure
RestartSec=5
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
```

Drop the `fluxdown-server` release binary into `/opt/fluxdown` (it carries the Web UI inside it — nothing else to install), create the `fluxdown` system user and `/var/lib/fluxdown`, then:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now fluxdown-server
sudo journalctl -u fluxdown-server -f   # watch for the first-run setup guidance banner
```

Then open `http://<host>:17800/` in a browser and complete the Initialize FluxDown Server wizard (or preset `FLUXDOWN_TOKEN` in the unit file for unattended setup).

## Next steps

- [Web UI](/docs/en/headless-server/web-ui/) — sign in and manage downloads from a browser.
- [API Overview](/docs/en/api/overview/) — automate the server from scripts or other tools.
