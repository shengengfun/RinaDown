# syntax=docker/dockerfile:1
# FluxDown headless 服务器镜像：单二进制 fluxdown-server（Web SPA 编译期内嵌）。
# 多架构：linux/amd64 + linux/arm64。编译阶段固定跑在构建机原生架构
# （--platform=$BUILDPLATFORM）并按 TARGETARCH 交叉编译，避免 QEMU 模拟下的
# Rust 全量编译（数小时级）；仅最终运行时层按目标架构拉取。
#
# 构建上下文 = 仓库根目录（依赖根 .dockerignore 收窄上下文）：
#   docker build -f docker/server.Dockerfile -t fluxdown-server .
#   docker buildx build --platform linux/amd64,linux/arm64 -f docker/server.Dockerfile .
# 发布构建注入版本号（/ping、/api/v1/stats、OpenAPI 显示；缺省退回 crate 版本）：
#   docker build -f docker/server.Dockerfile --build-arg FLUXDOWN_SERVER_VERSION=1.2.3 .
#
# 运行（首次打开 Web 界面时自行设置访问密钥；也可用 -e FLUXDOWN_TOKEN=... 预置）：
#   docker run -d -p 17800:17800 -v fluxdown-data:/data fluxdown-server

# ── Stage 1: Web 前端（Vite SPA，bun 锁文件；产物架构无关，跑在构建机架构）──
FROM --platform=$BUILDPLATFORM oven/bun:1 AS web
WORKDIR /src/web
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile
COPY web/ ./
# 云端 base URL 构建期烘焙进 SPA（与 release.yml build-web 的 VITE_FLUXCLOUD_BASE_URL
# 注入对称；缺省 = 本地开发回退 127.0.0.1:8720，见 web/src/lib/cloud/client.ts）。
ARG VITE_FLUXCLOUD_BASE_URL
ENV VITE_FLUXCLOUD_BASE_URL=$VITE_FLUXCLOUD_BASE_URL
RUN bun run build

# ── Stage 2: Rust 服务器（workspace 成员，仅编译 fluxdown_server，按 TARGETARCH 交叉编译）──
# Linux 侧全 rustls（无 openssl），SQLite 由 sqlx 捆绑编译（cc 交叉工具链），无额外系统依赖。
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS server
ARG TARGETARCH
WORKDIR /src
# aarch64 交叉链接器 / cc（libsqlite3-sys 等 build script 用）
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar
RUN case "$TARGETARCH" in \
      amd64) echo x86_64-unknown-linux-gnu > /rust-target ;; \
      arm64) echo aarch64-unknown-linux-gnu > /rust-target \
        && rustup target add aarch64-unknown-linux-gnu \
        && apt-get update \
        && apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
        && rm -rf /var/lib/apt/lists/* ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac
COPY Cargo.toml Cargo.lock ./
COPY native/ native/
# Web SPA 在编译期由 native/server/build.rs 嵌入二进制（单文件分发，运行时层
# 不再有 web/ 目录，也无需 FLUXDOWN_WEBROOT）。
COPY --from=web /src/web/dist /webroot
ENV FLUXDOWN_EMBED_WEBROOT=/webroot
# 编译期版本注入（空值 = 未注入，二进制退回 crate 版本）
ARG FLUXDOWN_SERVER_VERSION
ENV FLUXDOWN_SERVER_VERSION=$FLUXDOWN_SERVER_VERSION
# 编译期匿名统计 App-Key 注入（空值 = 未注入，统计整体禁用）
ARG FLUXDOWN_ANALYTICS_APP_KEY
ENV FLUXDOWN_ANALYTICS_APP_KEY=$FLUXDOWN_ANALYTICS_APP_KEY
# cache mount：本地重复构建增量编译；registry 缓存避免重复下载。
# id 必须按 TARGETARCH 隔离：buildx 多平台构建里 amd64/arm64 两个 server 阶段并发跑，
# 共享同一份 registry 缓存会让两个 cargo 同时解包同一个 crate（`.cargo-ok` File exists
# → error: failed to unpack package）。sharing=locked 再兜住同架构重试的并发。
RUN --mount=type=cache,id=fluxdown-cargo-registry-$TARGETARCH,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=fluxdown-cargo-target-$TARGETARCH,target=/src/target,sharing=locked \
    cargo build --release --locked -p fluxdown_server --target "$(cat /rust-target)" \
    && cp "target/$(cat /rust-target)/release/fluxdown-server" /usr/local/bin/fluxdown-server

# ── Stage 3: 运行时（目标架构 debian-slim + ca-certificates，rustls 读系统根证书）──
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server /usr/local/bin/fluxdown-server /app/fluxdown-server
# FLUXDOWN_BIND / FLUXDOWN_DATABASE_URL / FLUXDOWN_DEMO / FLUXDOWN_LANG 等见 native/server/src/config.rs
ENV FLUXDOWN_BIND=0.0.0.0:17800 \
    FLUXDOWN_DATA_DIR=/data
VOLUME /data
EXPOSE 17800
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
    CMD curl -fsS "http://127.0.0.1:${FLUXDOWN_BIND##*:}/ping" || exit 1
ENTRYPOINT ["/app/fluxdown-server"]
