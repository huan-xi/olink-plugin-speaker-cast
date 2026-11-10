#!/usr/bin/env bash
# 打包 speaker_cast .olinkpkg（独立仓，不依赖 olink workspace）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/dist/speaker_cast-1.0.0.olinkpkg}"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
esac

cd "$ROOT"
cargo build --release
BIN="$ROOT/target/release/olink-plugin-speaker-cast"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

mkdir -p "$STAGE/bin/${OS}-${ARCH}" "$STAGE/ui/icons"
cp "$BIN" "$STAGE/bin/${OS}-${ARCH}/plugin"
chmod 755 "$STAGE/bin/${OS}-${ARCH}/plugin"
cp "$ROOT/plugin.json" "$STAGE/plugin.json"
cp "$ROOT/ui/entry.js" "$STAGE/ui/entry.js"
cp "$ROOT/ui/entry.css" "$STAGE/ui/entry.css"
cp "$ROOT/ui/icons/icon.svg" "$STAGE/ui/icons/icon.svg"

mkdir -p "$(dirname "$OUT")"
rm -f "$OUT"
( cd "$STAGE" && zip -qr "$OUT" plugin.json bin ui )
echo "packed: $OUT"
ls -la "$OUT"
unzip -l "$OUT"
