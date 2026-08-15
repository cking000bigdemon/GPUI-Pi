# T1 静态验收（validate.sh 的 Windows 版）—— 每一轮 /loop 迭代结束都必须全绿。
#
#   .\scripts\validate.ps1          全量
#   .\scripts\validate.ps1 -Logic   只跑三个纯逻辑 crate
param([switch]$Logic)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Push-Location $Root

function Step($n, $name, $block) {
    Write-Host "### [$n/5] $name"
    & $block
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "$name 失败（exit $LASTEXITCODE）" }
}

try {
    if ($Logic) {
        $scope = @("-p", "pi-rpc", "-p", "pi-data", "-p", "pi-render")
        Write-Host "### 范围：仅纯逻辑 crate（pi-rpc / pi-data / pi-render）"
    } else {
        $scope = @("--workspace")
        Write-Host "### 范围：全工作区（含 gpui / gpui-component 编译）"
    }

    Step 1 "上游钉版本" { & "$Root\scripts\check-pins.ps1" }
    Step 2 "cargo fmt"  { cargo fmt --all -- --check }
    Step 3 "cargo clippy" { cargo clippy @scope --all-targets -- -D warnings }
    Step 4 "cargo test"   { cargo test @scope }
    Step 5 "cargo build --release" { cargo build --release @scope }

    Write-Host ""
    Write-Host "VALIDATE OK"
}
finally { Pop-Location }
