---
title: API Overview
description: The FluxDown HTTP API — route groups, authentication, and how it relates to the headless server.
section: api
order: 1
---

FluxDown ships a small HTTP API — used by browser extensions, userscripts, aria2 clients, and automation — built into two places:

- **The desktop app**, on `http://127.0.0.1:17800` (port configurable, address hardcoded to loopback: it is never reachable from the network). It's off by default for the management group and on by default for the other groups; see the desktop client's local API settings.
- **The [headless server](/docs/en/headless-server/setup/)**, on whatever address `FLUXDOWN_BIND` is set to (`0.0.0.0:17800` by default — reachable over the network by design, since remote management is the point). The management API is always enabled there, and it adds a handful of server-specific endpoints (queues, config, file retrieval, WebSocket, filesystem browsing) beyond what the desktop app exposes.

Both share the same underlying route constants, request/response JSON contracts, and auth rules — only which routes are enabled, and what host implements them, differs.

## Route groups

| Group | Endpoints | Enabled by | Authentication |
|---|---|---|---|
| Health check | `GET /ping` | always on | none |
| Script takeover | `POST /download`, `POST /download/batch` | `local_server_takeover_enabled` (default on) | `X-FluxDown-Client` header required, plus an optional token |
| aria2-compatible RPC | `POST /jsonrpc` (`aria2.addUri`, `aria2.getVersion`, `aria2.getGlobalStat`, `system.multicall`, `system.listMethods`) | `local_server_jsonrpc_enabled` (default on) | optional token |
| Management API | `GET /api/v1/info`, `GET/POST /api/v1/tasks`, `GET/DELETE /api/v1/tasks/{id}`, `PUT /api/v1/tasks/{id}/pause\|continue`, `PUT /api/v1/tasks/pause\|continue`, `GET /api/v1/queues` | `local_server_api_enabled` (default off on desktop, always on for the headless server) | **required** token |
| MCP | `POST /mcp` (`initialize`, `tools/list`, `tools/call`, `ping`) | `local_server_mcp_enabled` (default off on desktop, always on for the headless server) | **required** token (shared with the management API) |

`GET /api/v1/openapi.json` (no auth — it's a pure interface description with no data) is available whenever the management group is enabled.

On the headless server specifically, `/api/v1/*` also includes extra routes not present on the desktop app: `GET /api/v1/ws` (WebSocket), `GET/PUT /api/v1/config`, `POST/PUT/DELETE /api/v1/queues[/{id}]`, `POST /api/v1/queues/{id}/start|stop` (flip a queue between running and stopped — stopping pauses its tasks and excludes them from auto-start), `PUT /api/v1/queues/{id}/schedule` (daily start/stop times plus a weekday mask), `PUT /api/v1/queues/{id}/order` (persist the in-queue task start order), `PUT /api/v1/tasks/{id}/queue`, `PUT /api/v1/tasks/{id}/boost`, `GET /api/v1/tasks/{id}/file`, `GET /api/v1/fs/list`, `POST /api/v1/proxy/test`, `POST /api/v1/token/regenerate`, and `GET /api/v1/stats`. These follow the same token rules as the rest of the management group, except `/ws` and `/tasks/{id}/file` (browser-initiated requests can't set custom headers, so both take `?token=` as a query parameter instead) and `/openapi.json`/`/docs` (unauthenticated).

## Authentication

There is one configured token (`local_server_token`); how it must be presented depends on the route group:

| Route group | Accepted forms |
|---|---|
| Script takeover | `X-FluxDown-Token` header (only if a token is configured — empty token means the group is unauthenticated). The `X-FluxDown-Client` header is always required regardless of token, as a CORS-based gate against arbitrary web pages. |
| aria2-compatible RPC | `X-FluxDown-Token` header, **or** aria2's own convention of passing `token:xxx` as `params[0]` in the JSON-RPC call. |
| Management API (`/api/v1/*`) | `Authorization: Bearer <token>` **or** `X-FluxDown-Token` header. If no token is configured, every management request is rejected (403) — this group cannot run unauthenticated. |
| `/api/v1/ws`, `/api/v1/tasks/{id}/file` | `?token=<token>` query parameter (browser navigation/WebSocket upgrades can't set custom headers). |

Constant-time comparison is used everywhere a token is checked, to avoid timing side-channels.

### Cross-origin (CORS)

By default the service returns **no** `Access-Control-Allow-Origin` on any request, so a cross-origin `fetch()` from a web page is blocked at the preflight — that is precisely why requiring the `X-FluxDown-Client` header keeps arbitrary web pages out (userscripts use `GM_xmlhttpRequest`, which is not subject to CORS).

The **Allow cross-origin access from any website (CORS)** setting (`local_server_cors_allow_all`, off by default) gives that barrier up: preflight and real responses both carry `Access-Control-Allow-Origin: *`, and the preflight additionally carries `Access-Control-Allow-Private-Network: true` — the equivalent of aria2's `--rpc-allow-origin-all`. Its purpose is to let sites that probe for an aria2 service with a browser `fetch` detect FluxDown; the cost is that any web page can probe this port and submit download links. Still in effect: takeover and aria2 submissions raise the confirmation dialog on the desktop app, and the management API and MCP still require the token.

## Takeover/aria2 vs. management API: different semantics

`POST /download`, `/download/batch`, and `aria2.addUri` all funnel into the same "external download" path. **On the desktop app**, this pops a confirmation dialog before anything downloads — the assumption is a browser extension or userscript on an untrusted page is asking on the user's behalf, so a human confirms it. **On the headless server**, there's no UI to show a confirmation dialog, so the same entry points create the task directly, identically to the management API.

`POST /api/v1/tasks` (management API) always creates the task directly, with no confirmation — on both hosts. It assumes the caller is an already-authenticated, trusted automation client, not an untrusted web page acting through a userscript.

In short: on the desktop app, takeover/aria2 endpoints ask first and the management API doesn't; on the headless server, nothing asks first, because there's nobody to ask.

## curl examples

Create a task directly (management API):

```bash
curl -X POST http://<host>:17800/api/v1/tasks \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"url":"https://example.com/file.zip","segments":8}'
# -> {"taskId":"..."}
```

List tasks:

```bash
curl http://<host>:17800/api/v1/tasks \
  -H "Authorization: Bearer <token>"
```

Add a download via the aria2-compatible RPC (works with existing aria2-targeting userscripts/clients):

```bash
curl -X POST http://<host>:17800/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": "1",
    "method": "aria2.addUri",
    "params": ["token:<token>", ["https://example.com/file.zip"]]
  }'
```

`CreateTaskRequest` accepts `url` (required), and optional `fileName`, `saveDir`, `segments`, `cookies`, `referrer`, `proxyUrl`, `userAgent`, `queueId`, `checksum` (`algo=hexhash`), and `headers` — all camelCase in the JSON body. A supplied `fileName` is sanitized (path separators and `..` stripped) so the download always stays inside its save directory. The full schema is in the OpenAPI document below.

## MCP (Model Context Protocol)

FluxDown speaks [MCP](https://modelcontextprotocol.io) over HTTP, so AI clients (Claude Desktop, Cursor, Cline, and any MCP-capable agent) can drive downloads in natural language. It's a single endpoint, `POST /mcp`, protected by the same token as the management API.

MCP is JSON-RPC 2.0 over one HTTP endpoint (not REST) — every operation is one POST to `/mcp`, distinguished by the `method` in the body, using the stateless subset of the Streamable HTTP transport: requests get an `application/json` response, notifications get `202 Accepted`, and no session id is tracked. Authenticate with `Authorization: Bearer <token>` (or `X-FluxDown-Token`); the spec permits a static bearer token for internal deployments in place of OAuth 2.1.

### Tools

A client calls `tools/list` to discover these at runtime (each ships a full JSON Schema for its arguments), then `tools/call` to invoke one:

| Tool | What it does | Arguments |
|---|---|---|
| `download_add` | Create a download task (HTTP/HTTPS/FTP/magnet/BitTorrent). Returns the new task id. | `url` (required); optional `fileName`, `saveDir`, `segments`, `proxyUrl`, `cookies`, `referrer`, `userAgent`, `queueId`, `checksum` |
| `download_list` | List tasks, optionally filtered by status. | `status` (optional: `all`/`pending`/`downloading`/`paused`/`completed`/`error`/`preparing`) |
| `download_get` | Get one task's full detail by id. | `taskId` (required) |
| `download_pause` | Pause a task. | `taskId` (required) |
| `download_resume` | Resume a paused task. | `taskId` (required) |
| `download_pause_all` | Pause all active tasks (pending / downloading / preparing). | none |
| `download_resume_all` | Resume all paused tasks. | none |
| `download_remove` | Delete a task, optionally removing the file on disk. | `taskId` (required); optional `deleteFiles` (bool) |
| `queue_list` | List all named queues and their config. | none |

All nine map straight onto the management API's host capabilities, so an MCP client and a REST client see exactly the same tasks and queues.

### Connecting a client

Point your MCP client at the endpoint with a bearer token, e.g. in an `mcp.json`:

```json
{
  "mcpServers": {
    "fluxdown": {
      "url": "http://<host>:17800/mcp",
      "headers": { "Authorization": "Bearer <token>" }
    }
  }
}
```

Or exercise it directly with curl — initialize, then call a tool:

```bash
curl -X POST http://<host>:17800/mcp \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call",
       "params":{"name":"download_add",
                 "arguments":{"url":"https://example.com/file.zip","segments":8}}}'
```

## The fluxdown:// URL protocol

Alongside the HTTP API, FluxDown registers a custom URL protocol that any web page, script, or third-party app can use to hand a download off — no local HTTP call required:

```text
fluxdown://download?url=<percent-encoded URL>&filename=<optional name>
```

- `url` — required. The address to download, percent-encoded (`http`/`https`/`ftp` direct links or a `magnet:` link). A `fluxdown://` URL with a missing or empty `url` parameter is silently ignored.
- `filename` — optional. A suggested file name, pre-filled for the user to keep or change. Useful when the real name only exists in a `Content-Disposition` header the receiving app will never see.

Who answers it depends on the platform:

- **Desktop (Windows, macOS, Linux)** — the app registers the protocol handler (Windows registry on every startup; a `CFBundleURLTypes` declaration on macOS; an `x-scheme-handler` entry in the `.desktop` file on Linux). Opening a `fluxdown://` URL launches the app (or forwards to the already-running instance) and routes the request into the same external-download flow as browser-extension requests: a quick-download confirmation by default, silent task creation if the user enabled no-prompt downloads. On Android and in restricted desktop environments, the browser extension itself can deliver through this protocol — see [the fluxdown:// protocol mode](/docs/en/browser-extension/usage/).
- **Android** — the app declares a VIEW intent-filter for the scheme. Opening the URL wakes the app and shows the new-download sheet with `url` and `filename` pre-filled; the user confirms before anything downloads. Successive protocol URLs arriving while the sheet is open are merged into it as additional lines (this is how the browser extension delivers batch downloads on Android).

A plain HTML link is enough to integrate:

```html
<a href="fluxdown://download?url=https%3A%2F%2Fexample.com%2Ffile.zip&filename=file.zip">
  Download with FluxDown
</a>
```

Note the protocol carries no cookies, headers, or credentials — the receiving app fetches the URL from scratch. For authenticated downloads, use the script-takeover or management endpoints above, which accept `cookies` and `headers` in the request body.

## Interactive documentation

- [`/api-docs`](/api-docs) on this site renders the full OpenAPI 3.1 spec (generated from the actual route handlers) with a try-it-out UI, for the routes common to both hosts.
- A running headless server also serves its own live, merged spec (core + server-specific extension routes) at `/api/v1/docs` (Scalar UI) and `/api/v1/openapi.json` (raw JSON) — always in sync with the exact build you're running.
