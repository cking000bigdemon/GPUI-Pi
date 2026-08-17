#!/usr/bin/env bash
# 拉取钉死的 pi-web 参考源码（功能对照基线）到固定目录 vendor/upstream/pi-web-0.8.9/。
# 流程与 fetch-pi-source.sh 一致：API 验证远端 tag→commit（注解 tag 两级解析）→
# 归档 SHA256 校验 → 同卷临时目录解包 → 写 marker → 全量 manifest 比对 → 原子发布。
set -euo pipefail

PI_WEB_VERSION="0.8.9"
PI_WEB_TAG="v${PI_WEB_VERSION}"
PI_WEB_COMMIT="2a6e53710f6409e0cceb3de839a62f8cdf3ca3ca"
PI_WEB_SHA256="9624948a2194e51d6d99208ce74dcd648f4886654d167fefd0afd84588d44883"
REPO="agegr/pi-web"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/vendor/upstream/pi-web-${PI_WEB_VERSION}"
SOURCE_URL="https://codeload.github.com/${REPO}/tar.gz/refs/tags/${PI_WEB_TAG}"
CHECK="$ROOT/scripts/check-pi-web-pin.sh"

if [ -d "$DEST" ]; then
  if "$CHECK" "$DEST"; then
    echo "OK  vendor/upstream/pi-web-${PI_WEB_VERSION} 已存在且与基线一致 (${PI_WEB_TAG} @ ${PI_WEB_COMMIT})"
    exit 0
  fi
  echo "固定源码目录校验失败；如需重新准备请先删除：$DEST" >&2
  exit 1
fi

TMP_ROOT="$ROOT/vendor/upstream/.fetch-tmp-$$"
mkdir -p "$TMP_ROOT"
trap 'rm -rf "$TMP_ROOT"' EXIT

ARCHIVE="$TMP_ROOT/pi-web.tar.gz"
EXTRACT="$TMP_ROOT/extract"
mkdir -p "$EXTRACT"

echo "==> 验证远端 tag → commit（注解 tag 需两级解析；api.github.com 不可达时降级为警告）"
if ref_json="$(curl -fsS --connect-timeout 10 --max-time 30 \
    "https://api.github.com/repos/${REPO}/git/refs/tags/${PI_WEB_TAG}" 2>/dev/null)"; then
  obj_sha="$(printf '%s' "$ref_json" | grep -o '"sha": "[0-9a-f]\{40\}"' | head -n 1 | grep -o '[0-9a-f]\{40\}')"
  if printf '%s' "$ref_json" | grep -q '"type": "tag"'; then
    # v0.8.9 是注解 tag：refs 的 object 是 tag 对象，需再查 git/tags/{sha} 拿到 commit。
    tag_obj="$(curl -fsS --connect-timeout 10 --max-time 30 \
        "https://api.github.com/repos/${REPO}/git/tags/${obj_sha}" 2>/dev/null || true)"
    [ -n "$tag_obj" ] || { echo "FAIL 解析注解 tag 失败：${PI_WEB_TAG}" >&2; exit 1; }
    obj_sha="$(printf '%s' "$tag_obj" | grep -o '"sha": "[0-9a-f]\{40\}"' | head -n 1 | grep -o '[0-9a-f]\{40\}')"
  fi
  [ "$obj_sha" = "$PI_WEB_COMMIT" ] || { echo "FAIL 远端 ${PI_WEB_TAG} 指向 ${obj_sha}，钉死值为 ${PI_WEB_COMMIT}" >&2; exit 1; }
  echo "OK   远端 ${PI_WEB_TAG} -> ${PI_WEB_COMMIT}"
else
  echo "WARN api.github.com 不可达，跳过远端 tag 验证（归档 SHA256 仍是字节级证据）"
fi

echo "==> 下载 pi-web 源码 ${PI_WEB_TAG}（codeload tag 归档）"
curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 \
  -o "$ARCHIVE" "$SOURCE_URL"

echo "==> 校验归档 SHA256"
echo "${PI_WEB_SHA256}  $ARCHIVE" | sha256sum -c -

echo "==> 解包并写入 marker"
tar xzf "$ARCHIVE" -C "$EXTRACT"
SOURCE_ROOT="$EXTRACT/pi-web-${PI_WEB_VERSION}"
[ -d "$SOURCE_ROOT" ] || { echo "源码包根目录不是 pi-web-${PI_WEB_VERSION}" >&2; exit 1; }
printf 'version=%s\ntag=%s\ncommit=%s\narchive_sha256=%s\nsource=%s\n' \
  "$PI_WEB_VERSION" "$PI_WEB_TAG" "$PI_WEB_COMMIT" "$PI_WEB_SHA256" "$SOURCE_URL" \
  > "$SOURCE_ROOT/.gpui-pi-web-source-pin"

echo "==> 发布前全量校验（与 manifest 基线逐文件比对）"
"$CHECK" "$SOURCE_ROOT"

echo "==> 发布到固定目录"
mv "$SOURCE_ROOT" "$DEST"

echo "OK  vendor/upstream/pi-web-${PI_WEB_VERSION} (${PI_WEB_TAG} @ ${PI_WEB_COMMIT})"
