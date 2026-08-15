#!/usr/bin/env bash
# T1 静态验收 —— 每一轮 /loop 迭代结束都必须全绿才算完成。
#
#   ./scripts/validate.sh          全量（含 GPUI 编译，慢）
#   ./scripts/validate.sh --logic  只跑三个纯逻辑 crate（快，无需 GPU/系统库）
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

LOGIC_ONLY=0
[ "${1:-}" = "--logic" ] && LOGIC_ONLY=1

SCOPE=(--workspace)
if [ "$LOGIC_ONLY" = 1 ]; then
  SCOPE=(-p pi-rpc -p pi-data -p pi-render)
  echo "### 范围：仅纯逻辑 crate（pi-rpc / pi-data / pi-render）"
else
  echo "### 范围：全工作区（含 gpui / gpui-component 编译）"
fi

echo "### [1/5] 上游钉版本"
./scripts/check-pins.sh

echo "### [2/5] cargo fmt"
cargo fmt --all -- --check

echo "### [3/5] cargo clippy"
cargo clippy "${SCOPE[@]}" --all-targets -- -D warnings

echo "### [4/5] cargo test"
cargo test "${SCOPE[@]}"

echo "### [5/5] cargo build --release"
cargo build --release "${SCOPE[@]}"

echo
echo "VALIDATE OK"
