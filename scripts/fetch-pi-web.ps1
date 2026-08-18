# Fetch the pinned pi-web reference source (feature baseline) into a stable directory.
# Same flow as fetch-pi-source.ps1: verify remote tag -> commit via the GitHub API
# (annotated tags need a two-level lookup), verify archive SHA256, extract into a
# same-volume temp dir, write the pin marker, full manifest comparison, then publish.
$ErrorActionPreference = "Stop"

$PiWebVersion = "0.8.9"
$PiWebTag     = "v$PiWebVersion"
$PiWebCommit  = "2a6e53710f6409e0cceb3de839a62f8cdf3ca3ca"
$PiWebSha256  = "9624948a2194e51d6d99208ce74dcd648f4886654d167fefd0afd84588d44883"
$Repo         = "agegr/pi-web"
$Root         = Split-Path -Parent $PSScriptRoot
$Dest         = Join-Path $Root "vendor\upstream\pi-web-$PiWebVersion"
$SourceUrl    = "https://codeload.github.com/$Repo/tar.gz/refs/tags/$PiWebTag"
$Check        = Join-Path $Root "scripts\check-pi-web-pin.ps1"

if (Test-Path -LiteralPath $Dest -PathType Container) {
    & $Check -Dir $Dest
    if ($LASTEXITCODE -eq 0) {
        Write-Host "OK  vendor\upstream\pi-web-$PiWebVersion already exists and matches baseline ($PiWebTag @ $PiWebCommit)"
        exit 0
    }
    Write-Error "Stable reference directory failed verification; delete it first to re-fetch: $Dest"
    exit 1
}

$TmpRoot = Join-Path (Split-Path -Parent $Dest) (".fetch-tmp-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $TmpRoot | Out-Null

try {
    $Archive = Join-Path $TmpRoot "pi-web.tar.gz"
    $Extract = Join-Path $TmpRoot "extract"
    New-Item -ItemType Directory -Path $Extract | Out-Null

    Write-Host "==> Verify remote tag -> commit (annotated tag needs a two-level lookup; skips with a warning when api.github.com is unreachable)"
    try {
        $RefJson = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/git/refs/tags/$PiWebTag" -TimeoutSec 30 -UseBasicParsing
        $CommitSha = $RefJson.object.sha
        if ($RefJson.object.type -eq "tag") {
            $TagObj = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/git/tags/$CommitSha" -TimeoutSec 30 -UseBasicParsing
            $CommitSha = $TagObj.object.sha
        }
        if ($CommitSha -ne $PiWebCommit) {
            throw "Remote tag $PiWebTag points to $CommitSha, expected $PiWebCommit"
        }
        Write-Host "OK   remote $PiWebTag -> $PiWebCommit"
    }
    catch {
        Write-Warning "api.github.com unreachable; skipping remote tag verification (archive SHA256 still pins the bytes)"
    }

    Write-Host "==> Download pi-web source $PiWebTag (codeload tag archive)"
    Invoke-WebRequest -Uri $SourceUrl -OutFile $Archive -UseBasicParsing

    Write-Host "==> Verify archive SHA256"
    $GotSha256 = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLower()
    if ($GotSha256 -ne $PiWebSha256) {
        throw "Source archive SHA256 mismatch: expected $PiWebSha256, got $GotSha256"
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

    $SourceRoot = Join-Path $Extract "pi-web-$PiWebVersion"
    if (-not (Test-Path -LiteralPath $SourceRoot -PathType Container)) {
        throw "Unexpected source archive root; expected pi-web-$PiWebVersion"
    }
    @(
        "version=$PiWebVersion"
        "tag=$PiWebTag"
        "commit=$PiWebCommit"
        "archive_sha256=$PiWebSha256"
        "source=$SourceUrl"
    ) | Set-Content -LiteralPath (Join-Path $SourceRoot ".gpui-pi-web-source-pin") -Encoding ascii

    Write-Host "==> Full verification against baseline manifest before publish"
    & $Check -Dir $SourceRoot
    if ($LASTEXITCODE -ne 0) { throw "Pinned reference verification failed" }

    Write-Host "==> Publish to stable directory"
    Move-Item -LiteralPath $SourceRoot -Destination $Dest

    Write-Host "OK  vendor\upstream\pi-web-$PiWebVersion ($PiWebTag @ $PiWebCommit)"
}
finally {
    Remove-Item -LiteralPath $TmpRoot -Recurse -Force -ErrorAction SilentlyContinue
}
