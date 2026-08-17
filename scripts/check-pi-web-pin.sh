#!/usr/bin/env bash
# 校验固定的 pi-web 参考源码目录与钉死基线完全一致。供 check-pins.sh 与 fetch-pi-web.sh 调用。
#
# 用法：check-pi-web-pin.sh [dir]
#   dir 缺省为 vendor/upstream/pi-web-0.8.9；fetch 脚本发布前会传临时解包目录。
#
# 校验内容：
#   1. 目录存在且不含 .git；
#   2. .gpui-pi-web-source-pin marker 恰好 5 行，version/tag/commit/archive_sha256/source
#      逐行精确匹配；
#   3. 全部文件重新计算 SHA256 + 大小，与 pins/pi-web-0.8.9.manifest 基线逐行一致。
set -euo pipefail

PI_WEB_VERSION="0.8.9"
PI_WEB_TAG="v${PI_WEB_VERSION}"
PI_WEB_COMMIT="2a6e53710f6409e0cceb3de839a62f8cdf3ca3ca"
PI_WEB_SHA256="9624948a2194e51d6d99208ce74dcd648f4886654d167fefd0afd84588d44883"
PI_WEB_URL="https://codeload.github.com/agegr/pi-web/tar.gz/refs/tags/${PI_WEB_TAG}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="${1:-$ROOT/vendor/upstream/pi-web-${PI_WEB_VERSION}}"
MARKER="$SOURCE_DIR/.gpui-pi-web-source-pin"
MANIFEST="$ROOT/pins/pi-web-${PI_WEB_VERSION}.manifest"

fail=0
fail_msg() { echo "FAIL $*" >&2; fail=1; }

[ -f "$MANIFEST" ] || { echo "FAIL 缺少 manifest 基线：$MANIFEST" >&2; exit 1; }

if [ ! -d "$SOURCE_DIR" ]; then
  fail_msg "pi-web 参考源码未准备：运行 ./scripts/fetch-pi-web.sh"
  exit 1
fi

if [ -e "$SOURCE_DIR/.git" ]; then
  fail_msg "固定源码目录包含 .git，可能被 pull/checkout 改写：$SOURCE_DIR"
else
  echo "OK   pi-web 源码目录不含 .git"
fi

if [ -f "$MARKER" ]; then
  marker_lines="$(tr -d '\r' < "$MARKER" | grep -c . || true)"
  [ "$marker_lines" -eq 5 ] || fail_msg "marker 行数不是 5（实得 $marker_lines）：$MARKER"
  for pair in "version=${PI_WEB_VERSION}" "tag=${PI_WEB_TAG}" "commit=${PI_WEB_COMMIT}" \
              "archive_sha256=${PI_WEB_SHA256}" "source=${PI_WEB_URL}"; do
    tr -d '\r' < "$MARKER" | grep -qxF "$pair" || fail_msg "marker 缺少或字段不符：$pair"
  done
  echo "OK   pi-web 源码 marker（version/tag/commit/archive_sha256/source）"
else
  fail_msg "固定源码目录缺少 marker：$MARKER"
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
( cd "$SOURCE_DIR"
  find . -type f ! -name '.gpui-pi-web-source-pin' -print0 | xargs -0 sha256sum \
    | LC_ALL=C sort -k2 > "$TMP_DIR/hashes"
  find . -type f ! -name '.gpui-pi-web-source-pin' -print0 | xargs -0 stat -c '%s  %n' \
    | LC_ALL=C sort -k2 > "$TMP_DIR/sizes"
  awk 'NR==FNR { p=$2; sub(/^\*/,"",p); h[p]=$1; next }
       { printf "%s  %s  %s\n", h[$2], $1, substr($2,3) }' \
    "$TMP_DIR/hashes" "$TMP_DIR/sizes" | LC_ALL=C sort -k3 > "$TMP_DIR/manifest" )

if diff -q "$MANIFEST" "$TMP_DIR/manifest" >/dev/null; then
  echo "OK   pi-web 源码内容与 manifest 基线一致（$(wc -l < "$MANIFEST" | tr -d ' ') 个文件）"
else
  fail_msg "pi-web 源码内容与 manifest 基线不一致：$SOURCE_DIR"
  echo "     差异（左侧基线 / 右侧当前）：" >&2
  diff "$MANIFEST" "$TMP_DIR/manifest" | head -n 20 >&2 || true
fi

exit "$fail"
