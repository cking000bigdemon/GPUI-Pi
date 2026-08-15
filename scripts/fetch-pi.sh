#!/usr/bin/env bash
# 拉取钉死版本的 pi 独立二进制到 vendor/pi/。
#
# 版本钉死点之一 —— 另外两处是 crates/pi-rpc/src/lib.rs 的 PINNED_PI_VERSION
# 和 scripts/fetch-pi.ps1，三者由 pi-rpc 的单测强制同源。
set -euo pipefail

PI_VERSION="v0.84.2"
REPO="earendil-works/pi"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="$ROOT/vendor"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)   TARGET="linux-x64";   EXT="tar.gz" ;;
  Linux-aarch64)  TARGET="linux-arm64"; EXT="tar.gz" ;;
  Darwin-arm64)   TARGET="darwin-arm64"; EXT="tar.gz" ;;
  Darwin-x86_64)  TARGET="darwin-x64";  EXT="tar.gz" ;;
  *) echo "不支持的平台：$(uname -s)-$(uname -m)（Windows 请用 fetch-pi.ps1）" >&2; exit 1 ;;
esac

ASSET="pi-${TARGET}.${EXT}"
BASE="https://github.com/${REPO}/releases/download/${PI_VERSION}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> 下载 ${ASSET} (${PI_VERSION})"
curl -fsSL -o "$TMP/$ASSET" "$BASE/$ASSET"
curl -fsSL -o "$TMP/SHA256SUMS" "$BASE/SHA256SUMS"

echo "==> 校验 SHA256"
( cd "$TMP" && grep " ${ASSET}\$" SHA256SUMS | sha256sum -c - )

echo "==> 解包到 vendor/"
rm -rf "$VENDOR/pi"
mkdir -p "$VENDOR"
tar xzf "$TMP/$ASSET" -C "$VENDOR"
[ -d "$VENDOR/pi" ] || { echo "解包后没有 vendor/pi 目录" >&2; exit 1; }

echo "==> 自检"
GOT="$("$VENDOR/pi/pi" --version | tr -d '\r\n')"
WANT="${PI_VERSION#v}"
[ "$GOT" = "$WANT" ] || { echo "版本不符：期望 $WANT，实得 $GOT" >&2; exit 1; }

echo "OK  vendor/pi/pi  ($GOT)"
