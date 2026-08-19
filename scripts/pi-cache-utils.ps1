# 本机缓存工具 —— 供 fetch-pi.ps1 / fetch-pi-source.ps1 / fetch-pi-web.ps1 共用。
# 门禁每次在新 worktree 拉取钉死依赖都要走网络；缓存把「下载一次、反复使用」落到本机，
# 命中时只做本地校验 + 拷贝：
#   - 缓存根目录默认 D:\tmp\gpui-pi-cache，环境变量 GPUI_PI_CACHE 可覆盖路径，设 OFF 禁用；
#   - 缓存项必须通过与 vendor 同级的校验（manifest 全量比对 / SHA256）才允许被使用；
#   - 缓存缺失或校验失败时联网拉取，并在缓存目录内原子覆盖（临时目录 + rename）；
#   - 缓存目录位于仓库之外，vendor 里始终是每 worktree 独立的真实拷贝，不建任何链接
#     （对应 AGENTS.md 红线 6：禁止把共享目录链接进 worktree）。
$ErrorActionPreference = "Stop"

# 返回缓存根目录；不可用（禁用或建目录失败）时返回 $null，调用方应退回直接联网拉取。
function Get-PiCacheRoot {
    $envVal = $env:GPUI_PI_CACHE
    if ($envVal -and $envVal.Trim()) {
        if ($envVal.Trim().ToUpperInvariant() -eq "OFF") { return $null }
        $candidates = @($envVal.Trim())
    }
    else {
        $candidates = @("D:\tmp\gpui-pi-cache")
    }
    foreach ($c in $candidates) {
        try {
            New-Item -ItemType Directory -Path $c -Force -ErrorAction Stop | Out-Null
            return $c
        }
        catch {
            Write-Warning "cannot use cache dir $c ($($_.Exception.Message)); falling back to direct download"
        }
    }
    return $null
}

# 把已经过校验的临时文件/目录原子地发布进缓存：删旧项 + rename。
# 临时路径必须建在缓存根目录之下（同卷），Move-Item 才是原子 rename。
function Publish-PiCacheItem {
    param([string]$CacheRoot, [string]$Name, [string]$VerifiedPath)
    $Target = Join-Path $CacheRoot $Name
    if (Test-Path -LiteralPath $Target) { Remove-Item -LiteralPath $Target -Recurse -Force }
    Move-Item -LiteralPath $VerifiedPath -Destination $Target
}

# 确保缓存项存在且通过校验；否则执行 $Download（参数：临时目录，须返回已校验内容路径）
# 并在缓存目录内覆盖刷新，最多刷新两次。返回校验通过的缓存项路径。
function Ensure-PiCacheItem {
    param(
        [string]$CacheRoot,
        [string]$Name,
        [string]$CheckScript,
        [scriptblock]$Download
    )
    $Item = Join-Path $CacheRoot $Name
    for ($attempt = 0; $attempt -lt 2; $attempt++) {
        if (Test-Path -LiteralPath $Item -PathType Container) {
            & $CheckScript -Dir $Item
            if ($LASTEXITCODE -eq 0) { return $Item }
            Write-Warning "cache item $Name failed verification; re-fetching to overwrite (attempt $($attempt + 1))"
        }
        else {
            Write-Host "==> cache item $Name missing; fetching from network"
        }
        $TmpRoot = Join-Path $CacheRoot (".fetch-tmp-" + [guid]::NewGuid())
        New-Item -ItemType Directory -Path $TmpRoot | Out-Null
        try {
            $Verified = & $Download $TmpRoot
            if (-not $Verified -or -not (Test-Path -LiteralPath $Verified -PathType Container)) {
                throw "download block did not return a valid temp content path"
            }
            Publish-PiCacheItem -CacheRoot $CacheRoot -Name $Name -VerifiedPath $Verified
        }
        finally {
            Remove-Item -LiteralPath $TmpRoot -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    throw "cache item $Name still fails verification after two refreshes"
}
