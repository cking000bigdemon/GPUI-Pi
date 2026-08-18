# 拉取钉死版本的 pi 独立二进制到 vendor\pi\。
#
# 版本钉死点之一 —— 另外两处是 crates/pi-rpc/src/lib.rs 的 PINNED_PI_VERSION
# 和 scripts/fetch-pi.sh，三者由 pi-rpc 的单测强制同源。
$ErrorActionPreference = "Stop"

$PiVersion = "v0.84.2"
$Repo      = "earendil-works/pi"
$Root      = Split-Path -Parent $PSScriptRoot
$Vendor    = Join-Path $Root "vendor"

$Target = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "windows-x64" }
    "ARM64" { "windows-arm64" }
    default { throw "不支持的架构：$env:PROCESSOR_ARCHITECTURE" }
}

$Asset = "pi-$Target.zip"
$Base  = "https://github.com/$Repo/releases/download/$PiVersion"
$Tmp   = Join-Path ([System.IO.Path]::GetTempPath()) ("fetch-pi-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Tmp | Out-Null

try {
    Write-Host "==> 下载 $Asset ($PiVersion)"
    Invoke-WebRequest -Uri "$Base/$Asset"     -OutFile (Join-Path $Tmp $Asset)
    Invoke-WebRequest -Uri "$Base/SHA256SUMS" -OutFile (Join-Path $Tmp "SHA256SUMS")

    Write-Host "==> 校验 SHA256"
    $want = (Get-Content (Join-Path $Tmp "SHA256SUMS") |
             Where-Object { $_ -match "\s$([regex]::Escape($Asset))$" } |
             ForEach-Object { ($_ -split '\s+')[0] })
    if (-not $want) { throw "SHA256SUMS 里找不到 $Asset" }
    $got = (Get-FileHash (Join-Path $Tmp $Asset) -Algorithm SHA256).Hash.ToLower()
    if ($got -ne $want.ToLower()) { throw "SHA256 不符：期望 $want，实得 $got" }

    # 官方 zip 根目录直接是内容（pi.exe 在根、无顶层目录）；若未来包结构带顶层
    # 目录则自动改用该目录。解包目录建在 vendor 下、与目标同卷，保证最终
    # Move-Item 是原子 rename，不会跨卷退化成复制。
    New-Item -ItemType Directory -Path $Vendor -Force | Out-Null
    $Extract = Join-Path $Vendor (".fetch-pi-extract-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $Extract | Out-Null
    Expand-Archive -Path (Join-Path $Tmp $Asset) -DestinationPath $Extract -Force
    $Payload = Join-Path $Extract "pi"
    if (-not (Test-Path (Join-Path $Payload "pi.exe"))) { $Payload = $Extract }
    if (-not (Test-Path (Join-Path $Payload "pi.exe"))) { throw "解包后找不到 pi.exe" }

    Write-Host "==> 发布到 vendor\pi\"
    if (Test-Path (Join-Path $Vendor "pi")) { Remove-Item -Recurse -Force (Join-Path $Vendor "pi") }
    Move-Item -Path $Payload -Destination (Join-Path $Vendor "pi")

    Write-Host "==> 自检"
    $gotVer = (& (Join-Path $Vendor "pi\pi.exe") --version).Trim()
    $wantVer = $PiVersion.TrimStart("v")
    if ($gotVer -ne $wantVer) { throw "版本不符：期望 $wantVer，实得 $gotVer" }

    Write-Host "OK  vendor\pi\pi.exe  ($gotVer)"
}
finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
    if ($Extract) { Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue }
}
