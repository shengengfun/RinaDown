---
title: 服务器部署
description: 从源码构建并运行 headless FluxDown 服务器,了解全部环境变量并安全地对外暴露。
section: headless-server
order: 1
sourceHash: "5c2760a15f68"
---

`fluxdown_server` 是 FluxDown 下载引擎的 headless 版本:没有 Flutter 界面,也没有 Rinf/FFI 层。它把同一套 Rust 引擎(HTTP/HTTPS、FTP、BitTorrent、HLS、DASH)通过 HTTP、WebSocket 和一个编译进可执行文件的 Web 界面暴露出来,因此你可以把它跑在 NAS、家庭服务器或 VPS 上,在浏览器里远程管理下载。发行版就是**一个自包含的单二进制**,不用再附带 `web/` 目录。

多数部署场景下，预编译 Docker 镜像是最省事的方式——见 [Docker 与 NAS](/docs/zh/headless-server/docker/)。本页介绍从工作区源码用 Cargo 构建并运行，以及对两种方式都适用的配置。

## 构建与运行

服务器代码在 `native/server`(包名 `fluxdown_server`,可执行文件名 `fluxdown-server`)。在仓库根目录执行:

```bash
# 开发运行(debug 构建,默认监听 0.0.0.0:17800)
cargo run -p fluxdown_server

# 生产构建
cargo build --release -p fluxdown_server
# 产物路径:target/release/fluxdown-server(Windows 下为 fluxdown-server.exe)
```

进程本身是自包含的:它会打开自己的 SQLite(或 PostgreSQL)数据库、运行下载引擎,并直接从可执行文件内嵌的字节托管 Web 界面——不需要额外的数据库服务、静态文件目录或反向代理就能跑起来。

## 构建 Web 前端

只有自己编译服务器时才需要这一步——官方发行二进制与 Docker 镜像已经内含界面。

Web 界面是 `web/` 目录下独立的 SPA(React 19 + TanStack,用 [Bun](https://bun.sh) 构建)。它的构建产物在**编译期**被嵌入服务器二进制,所以必须在编译服务器**之前**就存在:

```bash
cd web
bun install
bun run build      # 输出到 web/dist

cd ..
cargo build --release -p fluxdown_server   # 把 web/dist 嵌进二进制
```

前端每次改动后都要重新编译服务器——正在运行的二进制永远只提供它编译时那份字节。两个编译期开关:

- `FLUXDOWN_EMBED_WEBROOT`——嵌入其它目录而不是 `web/dist`(CI 就用它,因为 SPA 在单独的 job 里构建)。
- 目录缺失或为空不会让编译失败,只打一条 warning;此时服务器对浏览器请求返回 `503` 说明页,REST API 与 WebSocket 照常工作。

<!-- TODO(screenshot): 浏览器里首次运行「初始化 FluxDown Server」向导的截图 -->

## 环境变量

全部配置在启动时从环境变量一次性读取,没有配置文件。

| 变量 | 默认值 | 说明 |
|---|---|---|
| `FLUXDOWN_BIND` | `0.0.0.0:17800` | HTTP/WebSocket 服务监听的 TCP 地址。 |
| `FLUXDOWN_DATA_DIR` | 平台自动探测(见下表) | 数据库文件与日志所在目录。 |
| `FLUXDOWN_DATABASE_URL` | 未设置——使用数据目录下的 SQLite 文件 | 显式连接串:`sqlite:/path/to/file.db` 或 `postgres://user:pass@host/db`。 |
| `FLUXDOWN_WEBROOT` | 未设置——托管内嵌的 Web 界面 | 可选覆盖:改从该目录托管 SPA,而不用内嵌那份(自定义前端,或热替换 `bun run build` 产物)。**不再**隐式探测可执行文件同级的 `./web`。 |
| `FLUXDOWN_TOKEN` | 未设置——走 Web 首次运行向导 | 可选的预置管理访问密钥。仅当库中尚未设置密钥时采纳(会 trim 首尾空白;须满足下文密钥规则,否则忽略并打警告)。用于 docker-compose / k8s / CI 等无人值守部署跳过向导。 |
| `FLUXDOWN_DEMO` | 未设置(关闭) | 真值(`1`/`true`/`yes`/`on`)开启演示模式:仅允许下载内置生成的 64 MiB 演示文件,适合公开演示。 |
| `FLUXDOWN_DEMO_URL` | 未设置(关闭) | 用指定 URL 覆盖演示模式的内置生成文件,仅该 URL 可下载。 |
| `FLUXDOWN_LANG` | 未设置(回退浏览器语言) | Web 界面默认语言(`en`/`zh`,接受 `zh-CN` 等区域变体)。纯回退值:任何用户在设置页保存过语言后,以保存值为服务器侧默认(实时生效,跨重启保留);在浏览器里显式选过语言的用户始终以本人选择为准。 |

未设置 `FLUXDOWN_DATA_DIR` 时,数据目录探测规则与桌面客户端一致:

| 平台 | 目录 |
|---|---|
| Windows(便携版) | 可执行文件同级目录 |
| Windows(安装版) | `%LOCALAPPDATA%\FluxDown\` |
| Linux | `$XDG_DATA_HOME/fluxdown/` |
| macOS | `~/Library/Application Support/fluxdown/` |

headless 部署几乎总是应该显式设置 `FLUXDOWN_DATA_DIR` 为一个固定、有备份的路径,而不是依赖自动探测。

```bash
FLUXDOWN_BIND=0.0.0.0:8080 \
FLUXDOWN_DATA_DIR=/srv/fluxdown/data \
./fluxdown-server
```

## 首次运行:在 Web 界面设置访问密钥

headless 服务器的管理 API 恒开(与桌面客户端默认关闭、需手动开启不同)。首次启动时,若库中尚未存有访问密钥,服务器进入**待设置**状态:所有管理端点(`/api/v1/*`、`/mcp`)返回 403,Web SPA 仍可访问,以便你在浏览器里完成初始化。

stderr 会打印中英双语引导横幅(不会生成密钥):

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

打开该地址。登录页会变成**初始化 FluxDown Server**向导(不是普通登录框):填写访问密钥并确认,可点按钮随机生成(`fxd_` + 24 位),可勾选「记住此设备」,保存后立即登录进主界面——无需重启服务器。

密钥规则(前后端一致):

- 仅 ASCII 可见字符(无空格、无非 ASCII)
- 长度 8–128
- 必须同时包含字母和数字

保存后密钥写入服务器自己数据库的 `config` 表,只要数据库文件(或 PostgreSQL 数据库)还在,重启后依然有效。用它来:

- 登录 Web 界面(见[Web 界面](/docs/zh/headless-server/web-ui/))。
- 用 `Authorization: Bearer <token>` 鉴权管理 API 调用(见 [API 总览](/docs/zh/api/overview/))。

这一流程取代了旧的「服务器生成 token 并只打印一次到 stderr」做法——因为 NAS(群晖、QNAP、Unraid 等)用户往往看不到容器/套件的 stderr,一次性打印的密钥等于把人锁在门外。

### 无人值守部署

若要跳过向导(docker-compose、Kubernetes、CI),用 `FLUXDOWN_TOKEN` 预置密钥。仅当库中还没有密钥时才会采纳:

```bash
FLUXDOWN_TOKEN='your-strong-key-here' ./fluxdown-server
```

### 安全提示

初始化窗口是「谁先访问谁落定」的一次性窗口。在把服务器暴露到不可信网络之前,应先完成初始化,或用 `FLUXDOWN_TOKEN` 预置。

### 重置访问密钥

如果密钥丢失或怀疑已泄露,可以在 Web 界面(**设置 → 安全与访问**)修改,或者用当前密钥鉴权后直接调用管理 API:

```bash
curl -X POST http://<host>:17800/api/v1/token/regenerate \
  -H "Authorization: Bearer <当前token>"
```

新密钥**立即生效**——旧密钥同刻失效,无需重启服务器。headless 服务器不允许清空访问密钥:通过 `PUT /api/v1/config` 写入 `local_server_token: ""` 会返回 400。

## 数据库:SQLite 默认,PostgreSQL 可选

默认情况下服务器会在数据目录里打开一个 SQLite 文件,无需任何设置。如果部署多实例或对吞吐量有更高要求,可以改用 PostgreSQL:

```bash
FLUXDOWN_DATABASE_URL=postgres://fluxdown:password@localhost/fluxdown \
cargo run -p fluxdown_server
```

连接串的 scheme(`sqlite:` 还是 `postgres:`)决定后端,两者共用同一套 schema 与迁移逻辑。服务器自己的日志会掩掉 `FLUXDOWN_DATABASE_URL` 里的凭证段,但这个环境变量本身仍要当作敏感信息对待(避免留在 shell 历史或明文提交到进程管理器配置里)。

## 安全地对外暴露(反向代理与 TLS)

`FLUXDOWN_BIND` 默认是 `0.0.0.0:17800`——监听所有网络接口,这与桌面客户端本机 API 硬编码只绑 `127.0.0.1` 不同。这是 headless 场景的刻意设计,但意味着**网络边界的安全由你负责**:

- 管理访问密钥是互联网与"完全远程控制你的服务器"(创建/删除下载、通过目录选择器浏览服务器文件系统、取回任意已完成文件)之间唯一的屏障。把它当 root 密码对待:不要分享、不要打进日志,一旦怀疑泄露就重新生成。
- 如果服务器需要在可信局域网之外访问,把它放在反向代理(nginx、Caddy、Traefik)之后终结 TLS,只对外暴露 HTTPS。Web 界面登录时密钥会出现在请求体/查询字符串里,明文 HTTP 下会被网络路径上的任何人看到。
- WebSocket 端点(`/api/v1/ws`)需要代理转发 `Upgrade`/`Connection` 头。最简 nginx 片段:

  ```nginx
  location / {
      proxy_pass http://127.0.0.1:17800;
      proxy_http_version 1.1;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "upgrade";
      proxy_set_header Host $host;
  }
  ```

- 相比直接把端口暴露给公网(即使配了 TLS),更推荐绑定到私有接口(`FLUXDOWN_BIND=127.0.0.1:17800`,由反向代理挡在前面)或 VPN/Tailscale 地址。

## 作为 systemd 服务运行

Linux 部署的最小 unit 文件示例(按需调整路径与用户):

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

把 `fluxdown-server`(release 二进制)放到 `/opt/fluxdown` 下(Web 界面就在它里面,没有别的文件要装),创建 `fluxdown` 系统用户与 `/var/lib/fluxdown` 目录,然后:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now fluxdown-server
sudo journalctl -u fluxdown-server -f   # 观察首次运行的引导横幅
```

随后在浏览器打开 `http://<host>:17800/` 完成「初始化 FluxDown Server」向导(无人值守部署可在 unit 里预置 `FLUXDOWN_TOKEN`)。

## 下一步

- [Web 界面](/docs/zh/headless-server/web-ui/)——在浏览器里登录并管理下载。
- [API 总览](/docs/zh/api/overview/)——用脚本或其它工具自动化操作服务器。
