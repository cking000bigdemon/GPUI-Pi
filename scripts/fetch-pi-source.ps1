# Fetch the pinned pi source into a stable directory outside auto-updated desktop app files.
# This source tree is read-only reference material; fetch-pi.ps1 still provides the runtime binary.
#
# 本机缓存：默认 D:\tmp\gpui-pi-cache，可用 GPUI_PI_CACHE 覆盖路径、设 OFF 禁用（CI 已禁用）。
# 命中缓存时只做本地校验 + 拷贝，不联网；缓存缺失/损坏才联网拉取并在缓存目录内覆盖更新。
# 发布流程：下载 codeload tag 归档 -> 校验归档 SHA256 -> 解压 -> 写 pin marker ->
# 全量 manifest 比对（check-pi-source-pin.ps1）-> 发布。缓存与 vendor 都只收已校验内容。
$ErrorActionPreference = "Stop"

$PiVersion = "0.84.2"
$PiTag     = "v$PiVersion"
$PiCommit  = "914cf1472e715297caa30db4b9535d534a9eb718"
$SourceSha256 = "65077457f18f9d3b0bc642870c5c19f41e38378e7f0ba4c3dd0962989e7d0036"
$Repo      = "earendil-works/pi"
$Root      = Split-Path -Parent $PSScriptRoot
$Dest      = Join-Path $Root "vendor\upstream\pi-$PiVersion"
$SourceUrl = "https://codeload.github.com/$Repo/tar.gz/refs/tags/$PiTag"
$Check     = Join-Path $Root "scripts\check-pi-source-pin.ps1"
. (Join-Path $Root "scripts\pi-cache-utils.ps1")

# 下载 + 校验 + 解压 + 写 marker + 全量比对，全部就绪才返回内容根目录（位于 $TmpRoot 下）。
$Download = {
    param([string]$TmpRoot)
    $Archive = Join-Path $TmpRoot "pi-source.tar.gz"
    $Extract = Join-Path $TmpRoot "extract"
    New-Item -ItemType Directory -Path $Extract | Out-Null

    Write-Host "==> Verify remote tag -> commit (skips with a warning when api.github.com is unreachable)"
    try {
        $TagJson = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/git/refs/tags/$PiTag" -TimeoutSec 30 -UseBasicParsing
        if ($TagJson.object.sha -ne $PiCommit) {
            throw "Remote tag $PiTag points to $($TagJson.object.sha), expected $PiCommit"
        }
        Write-Host "OK   remote $PiTag -> $PiCommit"
    }
    catch {
        Write-Warning "api.github.com unreachable; skipping remote tag verification (archive SHA256 still pins the bytes)"
    }

    Write-Host "==> Download pi source $PiTag (codeload tag archive)"
    # -UseBasicParsing keeps Windows PowerShell 5.1 from depending on the IE engine.
    Invoke-WebRequest -Uri $SourceUrl -OutFile $Archive -UseBasicParsing

    Write-Host "==> Verify archive SHA256"
    $GotSha256 = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLower()
    if ($GotSha256 -ne $SourceSha256) {
        throw "Source archive SHA256 mismatch: expected $SourceSha256, got $GotSha256"
    }

    Write-Host "==> Extract and write pin marker"
    # Git Bash 会把自己的 GNU tar.exe 放到 PATH 前面，并把 D:\... 误判为
    # remote:file。只在真正解压时定位 Windows 系统 tar；已准备目录的快速校验路径不受影响。
    $TarCandidates = @(
        (Join-Path $env:SystemRoot "Sysnative\tar.exe")
        (Join-Path $env:SystemRoot "System32\tar.exe")
    )
    $Tar = $TarCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
    if (-not $Tar) { throw "Windows system tar.exe not found" }
    & $Tar -xzf $Archive -C $Extract
    if ($LASTEXITCODE -ne 0) { throw "Windows system tar.exe failed with exit $LASTEXITCODE" }

    $SourceRoot = Join-Path $Extract "pi-$PiVersion"
    if (-not (Test-Path -LiteralPath $SourceRoot -PathType Container)) {
        throw "Unexpected source archive root; expected pi-$PiVersion"
    }
    @(
        "version=$PiVersion"
        "tag=$PiTag"
        "commit=$PiCommit"
        "archive_sha256=$SourceSha256"
        "source=$SourceUrl"
    ) | Set-Content -LiteralPath (Join-Path $SourceRoot ".gpui-pi-source-pin") -Encoding ascii

    Write-Host "==> Full verification against baseline manifest"
    & $Check -Dir $SourceRoot
    if ($LASTEXITCODE -ne 0) { throw "Pinned source verification failed" }

    return $SourceRoot
}

# 1) vendor 快路径：已存在且与基线逐字节一致，直接收工（不联网、不碰缓存）。
if (Test-Path -LiteralPath $Dest -PathType Container) {
    & $Check -Dir $Dest
    if ($LASTEXITCODE -eq 0) {
        Write-Host "OK  vendor\upstream\pi-$PiVersion already exists and matches baseline ($PiTag @ $PiCommit)"
        exit 0
    }
    Write-Warning "vendor\upstream\pi-$PiVersion failed verification; will re-publish from cache or network"
}

# 把已校验的源树拷贝成 vendor 目录并复验。vendor 内是真实拷贝，不建任何链接。
function Publish-PiSourceToVendor([string]$Source) {
    Write-Host "==> Publish to vendor\upstream\pi-$PiVersion"
    if (Test-Path -LiteralPath $Dest) { Remove-Item -LiteralPath $Dest -Recurse -Force }
    Copy-Item -LiteralPath $Source -Destination $Dest -Recurse
    & $Check -Dir $Dest
    if ($LASTEXITCODE -ne 0) { throw "published vendor tree failed verification: $Dest" }
}

# 2) 缓存路径：命中则本机拷贝；缺失/损坏则联网刷新缓存（在缓存目录内覆盖）。
$CacheRoot = Get-PiCacheRoot
if ($CacheRoot) {
    $CacheItem = Ensure-PiCacheItem -CacheRoot $CacheRoot -Name "pi-source-$PiVersion" `
        -CheckScript $Check -Download $Download
    Write-Host "==> cache hit; publishing from $CacheItem"
    Publish-PiSourceToVendor -Source $CacheItem
    Write-Host "OK  vendor\upstream\pi-$PiVersion ($PiTag @ $PiCommit)"
    exit 0
}

# 3) 兜底（缓存不可用）：维持原直连流程 —— 临时目录建在目标父目录下、同卷，
#    发布走同一个「拷贝 + 复验」函数。
$TmpRoot = Join-Path (Split-Path -Parent $Dest) (".fetch-tmp-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $TmpRoot | Out-Null
try {
    $Verified = & $Download $TmpRoot
    Publish-PiSourceToVendor -Source $Verified
    Write-Host "OK  vendor\upstream\pi-$PiVersion ($PiTag @ $PiCommit)"
}
finally {
    Remove-Item -LiteralPath $TmpRoot -Recurse -Force -ErrorAction SilentlyContinue
}
