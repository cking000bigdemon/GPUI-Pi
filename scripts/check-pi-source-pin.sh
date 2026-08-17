#!/usr/bin/env bash
# 校验固定源码目录与钉死基线完全一致。供 check-pins.sh 与 fetch-pi-source.sh 调用。
#
# 用法：check-pi-source-pin.sh [dir]
#   dir 缺省为 vendor/upstream/pi-0.84.2；fetch 脚本发布前会传临时解包目录。
#
# 校验内容：
#   1. 目录存在且不含 .git（不会被 pull/checkout 改写）；
#   2. .gpui-pi-source-pin marker 恰好 5 行，version/tag/commit/archive_sha256/source
#      逐行精确匹配（拒绝子串伪造、多余或缺失字段）；
#   3. 除 marker 外全部 1373 个文件重新计算 SHA256 + 大小，与 pins/pi-0.84.2.manifest
#      基线逐行一致 —— 任意文件增删改都会判红。
set -euo pipefail

PI_VERSION="0.84.2"
PI_TAG="v${PI_VERSION}"
PI_COMMIT="914cf1472e715297caa30db4b9535d534a9eb718"
SOURCE_SHA256="65077457f18f9d3b0bc642870c5c19f41e38378e7f0ba4c3dd0962989e7d0036"
SOURCE_URL="https://codeload.github.com/earendil-works/pi/tar.gz/refs/tags/${PI_TAG}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="${1:-$ROOT/vendor/upstream/pi-${PI_VERSION}}"
MARKER="$SOURCE_DIR/.gpui-pi-source-pin"
MANIFEST="$ROOT/pins/pi-${PI_VERSION}.manifest"

fail=0
fail_msg() { echo "FAIL $*" >&2; fail=1; }

[ -f "$MANIFEST" ] || { echo "FAIL 缺少 manifest 基线：$MANIFEST" >&2; exit 1; }

if [ ! -d "$SOURCE_DIR" ]; then
  fail_msg "pi 源码参考未准备：运行 ./scripts/fetch-pi-source.sh"
  exit 1
fi

if [ -e "$SOURCE_DIR/.git" ]; then
  fail_msg "固定源码目录包含 .git，可能被 pull/checkout 改写：$SOURCE_DIR"
else
  echo "OK   pi 源码目录不含 .git"
fi

# marker 精确校验：先剥 \r（Windows 侧 Set-Content 会写 CRLF），再逐行全等匹配。
if [ -f "$MARKER" ]; then
  marker_lines="$(tr -d '\r' < "$MARKER" | grep -c . || true)"
  [ "$marker_lines" -eq 5 ] || fail_msg "marker 行数不是 5（实得 $marker_lines）：$MARKER"
  for pair in "version=${PI_VERSION}" "tag=${PI_TAG}" "commit=${PI_COMMIT}" \
              "archive_sha256=${SOURCE_SHA256}" "source=${SOURCE_URL}"; do
    tr -d '\r' < "$MARKER" | grep -qxF "$pair" || fail_msg "marker 缺少或字段不符：$pair"
  done
  echo "OK   pi 源码 marker（version/tag/commit/archive_sha256/source）"
else
  fail_msg "固定源码目录缺少 marker：$MARKER"
fi

# 全量内容校验：重新生成 manifest 并与基线逐行比对。
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
( cd "$SOURCE_DIR"
  find . -type f ! -name '.gpui-pi-source-pin' -print0 | xargs -0 sha256sum \
    | LC_ALL=C sort -k2 > "$TMP_DIR/hashes"
  find . -type f ! -name '.gpui-pi-source-pin' -print0 | xargs -0 stat -c '%s  %n' \
    | LC_ALL=C sort -k2 > "$TMP_DIR/sizes"
  awk 'NR==FNR { p=$2; sub(/^\*/,"",p); h[p]=$1; next }
       { printf "%s  %s  %s\n", h[$2], $1, substr($2,3) }' \
    "$TMP_DIR/hashes" "$TMP_DIR/sizes" | LC_ALL=C sort -k3 > "$TMP_DIR/manifest" )

if diff -q "$MANIFEST" "$TMP_DIR/manifest" >/dev/null; then
  echo "OK   pi 源码内容与 manifest 基线一致（$(wc -l < "$MANIFEST" | tr -d ' ') 个文件）"
else
  fail_msg "pi 源码内容与 manifest 基线不一致：$SOURCE_DIR"
  echo "     差异（左侧基线 / 右侧当前）：" >&2
  diff "$MANIFEST" "$TMP_DIR/manifest" | head -n 20 >&2 || true
fi

exit "$fail"
