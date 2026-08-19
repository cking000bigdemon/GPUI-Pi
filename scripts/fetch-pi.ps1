# 拉取钉死版本的 pi 独立二进制到 vendor\pi\。
#
# 版本钉死点之一 —— 另外两处是 crates/pi-rpc/src/lib.rs 的 PINNED_PI_VERSION
# 和 scripts/fetch-pi.sh，三者由 pi-rpc 的单测强制同源。
#
# 本机缓存：默认 D:\tmp\gpui-pi-cache，可用 GPUI_PI_CACHE 覆盖路径、设 OFF 禁用（CI 已禁用）。
# 命中缓存时只做本地 SHA256 校验 + 解包，不联网；缓存缺失/损坏才联网下载并覆盖更新。
$ErrorActionPreference = "Stop"

$PiVersion = "v0.84.2"
$Repo      = "earendil-works/pi"
$Root      = Split-Path -Parent $PSScriptRoot
$Vendor    = Join-Path $Root "vendor"
. (Join-Path $Root "scripts\pi-cache-utils.ps1")

$Target = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "windows-x64" }
    "ARM64" { "windows-arm64" }
    default { throw "unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
}

$Asset        = "pi-$Target.zip"
$Base         = "https://github.com/$Repo/releases/download/$PiVersion"
$ZipCacheName = "bin-$Asset"                 # 缓存内 zip 名（按架构区分）
$SumCacheName = "SHA256SUMS-$PiVersion"      # 校验和清单按版本区分

# 校验 zip 与 SHA256SUMS 是否一致（本地哈希，不联网）。
function Test-PiBinCache([string]$ZipPath, [string]$SumsPath, [string]$AssetName) {
    $want = (Get-Content -LiteralPath $SumsPath |
             Where-Object { $_ -match "\s$([regex]::Escape($AssetName))$" } |
             ForEach-Object { ($_ -split '\s+')[0] })
    if (-not $want) { return $false }
    $got = (Get-FileHash -LiteralPath $ZipPath -Algorithm SHA256).Hash.ToLower()
    return ($got -eq $want.ToLower())
}

# 从（已校验的）zip 解包发布到 vendor\pi\ 并自检版本。
function Publish-PiBinFromZip([string]$ZipPath, [string]$AssetName) {
    New-Item -ItemType Directory -Path $Vendor -Force | Out-Null
    # 解包目录建在 vendor 下、与目标同卷，保证最终 Move-Item 是原子 rename，
    # 不会跨卷退化成复制。
    $Extract = Join-Path $Vendor (".fetch-pi-extract-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $Extract | Out-Null
    try {
        Expand-Archive -Path $ZipPath -DestinationPath $Extract -Force
        # 官方 zip 根目录直接是内容（pi.exe 在根、无顶层目录）；若未来包结构带顶层
        # 目录则自动改用该目录。
        $Payload = Join-Path $Extract "pi"
        if (-not (Test-Path (Join-Path $Payload "pi.exe"))) { $Payload = $Extract }
        if (-not (Test-Path (Join-Path $Payload "pi.exe"))) { throw "pi.exe not found after extraction" }

        Write-Host "==> Publish to vendor\pi\"
        if (Test-Path (Join-Path $Vendor "pi")) { Remove-Item -Recurse -Force (Join-Path $Vendor "pi") }
        Move-Item -Path $Payload -Destination (Join-Path $Vendor "pi")

        Write-Host "==> Self check"
        $gotVer = (& (Join-Path $Vendor "pi\pi.exe") --version).Trim()
        $wantVer = $PiVersion.TrimStart("v")
        if ($gotVer -ne $wantVer) { throw "version mismatch: expected $wantVer, got $gotVer" }

        Write-Host "OK  vendor\pi\pi.exe  ($gotVer)"
    }
    finally {
        Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue
    }
}

# 1) vendor 快路径：已有 pi.exe 且版本吻合，直接收工（原脚本每轮都重下 100MB+）。
$Exe = Join-Path $Vendor "pi\pi.exe"
if (Test-Path -LiteralPath $Exe -PathType Leaf) {
    $gotVer = (& $Exe --version).Trim()
    if ($gotVer -eq $PiVersion.TrimStart("v")) {
        Write-Host "OK  vendor\pi\pi.exe already exists ($gotVer)"
        exit 0
    }
    Write-Warning "vendor\pi\pi.exe version mismatch ($gotVer); will re-publish"
}

# 2) 缓存路径：命中（SHA256 一致）则直接解包；缺失/损坏则联网下载并覆盖缓存。
$CacheRoot = Get-PiCacheRoot
if ($CacheRoot) {
    $ZipPath  = Join-Path $CacheRoot $ZipCacheName
    $SumsPath = Join-Path $CacheRoot $SumCacheName
    $ready = $false
    for ($attempt = 0; $attempt -lt 2; $attempt++) {
        if ((Test-Path -LiteralPath $ZipPath -PathType Leaf) -and (Test-Path -LiteralPath $SumsPath -PathType Leaf)) {
            if (Test-PiBinCache -ZipPath $ZipPath -SumsPath $SumsPath -AssetName $Asset) { $ready = $true; break }
            Write-Warning "cached zip failed SHA256 check; re-downloading to overwrite (attempt $($attempt + 1))"
        }
        else {
            Write-Host "==> cached zip missing; downloading from network"
        }
        # 临时文件建在缓存根目录下，同卷 rename 原子覆盖。
        $TmpRoot = Join-Path $CacheRoot (".fetch-tmp-" + [guid]::NewGuid())
        New-Item -ItemType Directory -Path $TmpRoot | Out-Null
        try {
            Write-Host "==> Download $Asset ($PiVersion)"
            Invoke-WebRequest -Uri "$Base/$Asset"     -OutFile (Join-Path $TmpRoot $Asset)
            Invoke-WebRequest -Uri "$Base/SHA256SUMS" -OutFile (Join-Path $TmpRoot "SHA256SUMS")

            Write-Host "==> Verify SHA256"
            $want = (Get-Content (Join-Path $TmpRoot "SHA256SUMS") |
                     Where-Object { $_ -match "\s$([regex]::Escape($Asset))$" } |
                     ForEach-Object { ($_ -split '\s+')[0] })
            if (-not $want) { throw "$Asset not found in SHA256SUMS" }
            $got = (Get-FileHash (Join-Path $TmpRoot $Asset) -Algorithm SHA256).Hash.ToLower()
            if ($got -ne $want.ToLower()) { throw "SHA256 mismatch: expected $want, got $got" }

            Publish-PiCacheItem -CacheRoot $CacheRoot -Name $ZipCacheName -VerifiedPath (Join-Path $TmpRoot $Asset)
            Publish-PiCacheItem -CacheRoot $CacheRoot -Name $SumCacheName -VerifiedPath (Join-Path $TmpRoot "SHA256SUMS")
        }
        finally {
            Remove-Item -LiteralPath $TmpRoot -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    if (-not $ready) { throw "pi binary cache still fails verification after two refreshes" }

    Write-Host "==> cache hit; publishing from cached zip"
    Publish-PiBinFromZip -ZipPath $ZipPath -AssetName $Asset
    exit 0
}

# 3) 兜底（缓存不可用）：维持原直连流程。
$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("fetch-pi-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    Write-Host "==> Download $Asset ($PiVersion)"
    Invoke-WebRequest -Uri "$Base/$Asset"     -OutFile (Join-Path $Tmp $Asset)
    Invoke-WebRequest -Uri "$Base/SHA256SUMS" -OutFile (Join-Path $Tmp "SHA256SUMS")

    Write-Host "==> Verify SHA256"
    $want = (Get-Content (Join-Path $Tmp "SHA256SUMS") |
             Where-Object { $_ -match "\s$([regex]::Escape($Asset))$" } |
             ForEach-Object { ($_ -split '\s+')[0] })
    if (-not $want) { throw "$Asset not found in SHA256SUMS" }
    $got = (Get-FileHash (Join-Path $Tmp $Asset) -Algorithm SHA256).Hash.ToLower()
    if ($got -ne $want.ToLower()) { throw "SHA256 mismatch: expected $want, got $got" }

    Publish-PiBinFromZip -ZipPath (Join-Path $Tmp $Asset) -AssetName $Asset
}
finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
