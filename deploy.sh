#!/bin/bash
# deploy.sh — sync source to RPi and build there.
#
# Usage:
#   ./deploy.sh              # sync + build
#   ./deploy.sh --sync-only  # sync only (no build)
#   ./deploy.sh --build-only # build only (no sync)
#
# Configuration: override via environment variables or edit the defaults below.
#   RPI_HOST=pi.local RPI_USER=pi ./deploy.sh

set -euo pipefail

RPI_HOST="${RPI_HOST:-rpi3}"
RPI_DIR="${RPI_DIR:-~/led-service2}"
BUILD_TARGET="${BUILD_TARGET:-aarch64-unknown-linux-gnu}"

SYNC=true
BUILD=true
for arg in "$@"; do
  case "$arg" in
    --sync-only)  BUILD=false ;;
    --build-only) SYNC=false ;;
    *) echo "Unknown option: $arg"; exit 1 ;;
  esac
done

if $SYNC; then
  echo "==> Syncing source to ${RPI_HOST}:${RPI_DIR} ..."
  rsync -az --delete \
    --exclude target/ \
    --exclude .git/ \
    . "${RPI_HOST}:${RPI_DIR}"
  echo "    Sync done."
fi

if $BUILD; then
  echo "==> Building on RPi (make build) ..."
  # shellcheck disable=SC2029
  ssh "${RPI_HOST}" "bash -l -c 'set -e; cd ${RPI_DIR} && make build'"
  echo "    Build done."
  echo ""
  echo "Binary: ${RPI_DIR}/target/release/led-server"
fi
