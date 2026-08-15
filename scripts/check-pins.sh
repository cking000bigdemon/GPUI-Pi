#!/usr/bin/env bash
# 校验 Cargo.lock 里的上游 sha 与立项文档 § 二 钉死的一致。
#
# gpui / gpui_platform 的 git 依赖不带 rev（必须与 gpui-component 的写法一致，
# 否则 cargo 会因同一 URL 两个 reference 而拒绝解析），所以真正的钉死点是提交
# 进仓库的 Cargo.lock。这个脚本就是那把锁 —— 谁 `cargo update` 谁红。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCK="$ROOT/Cargo.lock"

ZED_SHA="cc053a4a6fa2fd0e8793201ed9099466af1be0b1"
GPUIC_SHA="000114aad412b1a1b26cb65cd0c8ae9467fd396a"

fail=0
check() {
  local label="$1" needle="$2"
  if grep -qF "$needle" "$LOCK"; then
    echo "OK   $label"
  else
    echo "FAIL $label —— Cargo.lock 里找不到 $needle" >&2
    fail=1
  fi
}

# 只检查"钉的 sha 在场"是不够的：cargo update 会把同一个 git 源里的一部分包
# 挪到新 sha、留一部分在旧 sha，形成半新半旧的混合锁 —— R0 实测踩过。
# 所以还要检查"没有第二个 sha"。
check_no_stray() {
  local label="$1" url="$2" want="$3"
  local stray
  stray="$(grep -oE "git\+${url}#[0-9a-f]{40}" "$LOCK" | sort -u | grep -v "#${want}\$" || true)"
  if [ -z "$stray" ]; then
    echo "OK   $label 无杂散 sha"
  else
    echo "FAIL $label 出现了别的 sha：" >&2
    echo "$stray" >&2
    fail=1
  fi
}

[ -f "$LOCK" ] || { echo "FAIL 没有 Cargo.lock（先跑一次 cargo generate-lockfile）" >&2; exit 1; }

check "zed (gpui / gpui_platform)" "git+https://github.com/zed-industries/zed#${ZED_SHA}"
check "gpui-component"             "git+https://github.com/longbridge/gpui-component#${GPUIC_SHA}"
check_no_stray "zed"            "https://github\.com/zed-industries/zed"      "$ZED_SHA"
check_no_stray "gpui-component" "https://github\.com/longbridge/gpui-component" "$GPUIC_SHA"

exit "$fail"
