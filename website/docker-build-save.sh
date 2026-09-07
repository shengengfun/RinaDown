#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# docker-build-save.sh
# 构建 fluxdown-website Docker 镜像并将其保存为 .tar 文件
#
# 用法:
#   ./docker-build-save.sh [TAG] [OUTPUT_FILE]
#
# 参数:
#   TAG         镜像标签，默认为 latest
#   OUTPUT_FILE 输出文件路径，默认为 fluxdown-website-<TAG>.tar
#
# 示例:
#   ./docker-build-save.sh
#   ./docker-build-save.sh v1.0.0
#   ./docker-build-save.sh v1.0.0 /tmp/my-image.tar
# ─────────────────────────────────────────────────────────────
set -euo pipefail

IMAGE_NAME="fluxdown-website"
TAG="${1:-latest}"
FULL_IMAGE="${IMAGE_NAME}:${TAG}"
OUTPUT_FILE="${2:-${IMAGE_NAME}-${TAG}.tar}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=========================================="
echo "  FluxDown Website — Docker Build & Save"
echo "=========================================="
echo "  镜像名称 : ${FULL_IMAGE}"
echo "  输出文件 : ${OUTPUT_FILE}"
echo "  构建上下文 : ${SCRIPT_DIR}"
echo "------------------------------------------"

# ── 1. 构建镜像 ──────────────────────────────
echo ""
echo "[1/2] 正在构建 Docker 镜像..."
docker build \
  --file "${SCRIPT_DIR}/Dockerfile" \
  --tag "${FULL_IMAGE}" \
  "${SCRIPT_DIR}"

echo "      ✓ 镜像构建完成: ${FULL_IMAGE}"

# ── 2. 保存镜像到文件 ────────────────────────
echo ""
echo "[2/2] 正在保存镜像到文件..."
docker save \
  --output "${OUTPUT_FILE}" \
  "${FULL_IMAGE}"

FILE_SIZE="$(du -sh "${OUTPUT_FILE}" | cut -f1)"
echo "      ✓ 镜像已保存: ${OUTPUT_FILE} (${FILE_SIZE})"

echo ""
echo "=========================================="
echo "  完成！"
echo "------------------------------------------"
echo "  加载镜像命令:"
echo "    docker load -i ${OUTPUT_FILE}"
echo "  运行镜像命令:"
echo "    docker run -p 4321:4321 ${FULL_IMAGE}"
echo "  或使用 docker-compose:"
echo "    docker-compose up -d"
echo "=========================================="
