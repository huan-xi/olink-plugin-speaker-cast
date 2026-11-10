#!/usr/bin/env bash
# 打包 speaker_cast .olinkpkg
# 目录名必须对齐 Host：std::env::consts::{OS,ARCH} → bin/{os}-{arch}/plugin
#   macos / linux / windows × x86_64 / aarch64
#
# 用法：
#   ./scripts/pack.sh                         # 仅本机 Host 架构
#   PACK_TARGETS=linux-aarch64,macos-aarch64 ./scripts/pack.sh
#   ./scripts/pack.sh dist/out.olinkpkg
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/dist/speaker_cast-1.0.0.olinkpkg}"
BIN_NAME="olink-plugin-speaker-cast"

# uname → Host consts（Darwin ≠ macos）
host_os() {
  case "$(uname -s | tr '[:upper:]' '[:lower:]')" in
    darwin) echo macos ;;
    linux) echo linux ;;
    mingw*|msys*|cygwin*) echo windows ;;
    *) echo "$(uname -s | tr '[:upper:]' '[:lower:]')" ;;
  esac
}
host_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo x86_64 ;;
    aarch64|arm64) echo aarch64 ;;
    *) echo "$(uname -m)" ;;
  esac
}

# Host 标签 → rustc target triple（交叉编时用）
triple_for() {
  case "$1" in
    linux-x86_64) echo x86_64-unknown-linux-gnu ;;
    linux-aarch64) echo aarch64-unknown-linux-gnu ;;
    macos-x86_64) echo x86_64-apple-darwin ;;
    macos-aarch64) echo aarch64-apple-darwin ;;
    windows-x86_64) echo x86_64-pc-windows-msvc ;;
    windows-aarch64) echo aarch64-pc-windows-msvc ;;
    *)
      echo "unknown PACK_TARGETS entry: $1" >&2
      return 1
      ;;
  esac
}

HOST_TAG="$(host_os)-$(host_arch)"
if [[ -n "${PACK_TARGETS:-}" ]]; then
  IFS=',' read -r -a TARGETS <<< "${PACK_TARGETS}"
else
  TARGETS=("$HOST_TAG")
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/ui/icons"

cd "$ROOT"
for tag in "${TARGETS[@]}"; do
  tag="$(echo "$tag" | tr -d '[:space:]')"
  [[ -z "$tag" ]] && continue
  triple="$(triple_for "$tag")"
  echo "build $tag ($triple)"
  if [[ "$tag" == "$HOST_TAG" ]]; then
    cargo build --release
    src="$ROOT/target/release/$BIN_NAME"
    # windows host
    [[ -f "${src}.exe" ]] && src="${src}.exe"
  else
    rustup target add "$triple" >/dev/null 2>&1 || true
    cargo build --release --target "$triple"
    src="$ROOT/target/$triple/release/$BIN_NAME"
    [[ -f "${src}.exe" ]] && src="${src}.exe"
  fi
  if [[ ! -f "$src" ]]; then
    echo "missing binary: $src" >&2
    exit 1
  fi
  mkdir -p "$STAGE/bin/$tag"
  cp "$src" "$STAGE/bin/$tag/plugin"
  chmod 755 "$STAGE/bin/$tag/plugin"
done

cp "$ROOT/plugin.json" "$STAGE/plugin.json"
cp "$ROOT/ui/entry.js" "$STAGE/ui/entry.js"
cp "$ROOT/ui/entry.css" "$STAGE/ui/entry.css"
cp "$ROOT/ui/icons/icon.svg" "$STAGE/ui/icons/icon.svg"

mkdir -p "$(dirname "$OUT")"
rm -f "$OUT"
( cd "$STAGE" && zip -qr "$OUT" plugin.json bin ui )
echo "packed: $OUT (host would pick bin/${HOST_TAG}/plugin)"
ls -la "$OUT"
unzip -l "$OUT"
