#!/usr/bin/env bash
# 拉取钉死版本的 pi 源码到不会被桌面应用自动更新覆盖的固定目录。
# 这份源码只作只读参考；运行时二进制仍由 fetch-pi.sh 准备。
#
# 发布流程：下载 codeload tag 归档 → 校验归档 SHA256 → 解包到 vendor/upstream/
# 下的临时目录（与目标同卷，保证最后 mv 是原子 rename）→ 写入 marker →
# 用 check-pi-source-pin.sh 全量比对 manifest 基线 → 通过才发布。
set -euo pipefail

PI_VERSION="0.84.2"
PI_TAG="v${PI_VERSION}"
PI_COMMIT="914cf1472e715297caa30db4b9535d534a9eb718"
SOURCE_SHA256="65077457f18f9d3b0bc642870c5c19f41e38378e7f0ba4c3dd0962989e7d0036"
REPO="earendil-works/pi"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/vendor/upstream/pi-${PI_VERSION}"
SOURCE_URL="https://codeload.github.com/${REPO}/tar.gz/refs/tags/${PI_TAG}"
CHECK="$ROOT/scripts/check-pi-source-pin.sh"

if [ -d "$DEST" ]; then
  if "$CHECK" "$DEST"; then
    echo "OK  vendor/upstream/pi-${PI_VERSION} 已存在且与基线一致 (${PI_TAG} @ ${PI_COMMIT})"
    exit 0
  fi
  echo "固定源码目录校验失败；如需重新准备请先删除：$DEST" >&2
  exit 1
fi

# 临时目录建在目标父目录下，保证最后 mv 在同一文件系统内是原子 rename；
# 系统临时目录（通常 C:）与仓库盘（D:）跨卷时 mv 会退化成复制，中断会留下半份目录。
TMP_ROOT="$ROOT/vendor/upstream/.fetch-tmp-$$"
mkdir -p "$TMP_ROOT"
trap 'rm -rf "$TMP_ROOT"' EXIT

ARCHIVE="$TMP_ROOT/pi-source.tar.gz"
EXTRACT="$TMP_ROOT/extract"
mkdir -p "$EXTRACT"

echo "==> 验证远端 tag → commit（api.github.com 不可达时降级为警告）"
if tag_json="$(curl -fsS --connect-timeout 10 --max-time 30 \
    "https://api.github.com/repos/${REPO}/git/refs/tags/${PI_TAG}" 2>/dev/null)"; then
  printf '%s' "$tag_json" | grep -qF "\"sha\": \"${PI_COMMIT}\"" \
    || { echo "远端 tag ${PI_TAG} 指向的 commit 与钉死值不符" >&2; exit 1; }
  echo "OK   远端 ${PI_TAG} -> ${PI_COMMIT}"
else
  echo "WARN api.github.com 不可达，跳过远端 tag 验证（归档 SHA256 仍是字节级证据）"
fi

echo "==> 下载 pi 源码 ${PI_TAG}（codeload tag 归档）"
curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 \
  -o "$ARCHIVE" "$SOURCE_URL"

echo "==> 校验归档 SHA256"
echo "${SOURCE_SHA256}  $ARCHIVE" | sha256sum -c -

echo "==> 解包并写入 marker"
tar xzf "$ARCHIVE" -C "$EXTRACT"
SOURCE_ROOT="$EXTRACT/pi-${PI_VERSION}"
[ -d "$SOURCE_ROOT" ] || { echo "源码包根目录不是 pi-${PI_VERSION}" >&2; exit 1; }
printf 'version=%s\ntag=%s\ncommit=%s\narchive_sha256=%s\nsource=%s\n' \
  "$PI_VERSION" "$PI_TAG" "$PI_COMMIT" "$SOURCE_SHA256" "$SOURCE_URL" \
  > "$SOURCE_ROOT/.gpui-pi-source-pin"

echo "==> 发布前全量校验（与 manifest 基线逐文件比对）"
"$CHECK" "$SOURCE_ROOT"

echo "==> 发布到固定目录"
mv "$SOURCE_ROOT" "$DEST"

echo "OK  vendor/upstream/pi-${PI_VERSION} (${PI_TAG} @ ${PI_COMMIT})"
