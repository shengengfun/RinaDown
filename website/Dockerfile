# ─────────────────────────────────────────────
# Stage 1: Build
# ─────────────────────────────────────────────
FROM node:22-alpine AS builder

WORKDIR /app

# 仅先复制依赖声明，利用 Docker layer 缓存
# 本地用 bun/pnpm，不维护 package-lock.json，故用 install 而非 ci（lock 文件可有可无）
COPY package.json ./

RUN npm install

# 复制源码
COPY . .

# 构建时不需要 .env，环境变量在运行时注入
# 但 Astro 在构建时会读取 import.meta.env 中的公开变量（PUBLIC_ 前缀）
# 本项目所有 env 变量均为服务端运行时变量，构建时无需注入
RUN npm run build

# ─────────────────────────────────────────────
# Stage 2: Production
# ─────────────────────────────────────────────
FROM node:22-alpine AS runtime

WORKDIR /app

ENV NODE_ENV=production
ENV HOST=0.0.0.0
ENV PORT=4321

# 直接复用 builder 阶段已安装的 node_modules（避免重复安装）
COPY --from=builder /app/dist ./dist
COPY --from=builder /app/node_modules ./node_modules

EXPOSE 4321

# @astrojs/node standalone 模式的入口
CMD ["node", "./dist/server/entry.mjs"]
