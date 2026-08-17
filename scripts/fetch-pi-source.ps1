# Fetch the pinned pi source into a stable directory outside auto-updated desktop app files.
# This source tree is read-only reference material; fetch-pi.ps1 still provides the runtime binary.
#
# Publish flow: download the codeload tag archive -> verify archive SHA256 -> extract into a
# temp dir under vendor\upstream (same volume as the target, so the final move is an atomic
# rename) -> write the pin marker -> full manifest comparison via check-pi-source-pin.ps1
# -> publish only after everything matches.
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

if (Test-Path -LiteralPath $Dest -PathType Container) {
    & $Check -Dir $Dest
    if ($LASTEXITCODE -eq 0) {
        Write-Host "OK  vendor\upstream\pi-$PiVersion already exists and matches baseline ($PiTag @ $PiCommit)"
        exit 0
    }
    Write-Error "Stable source directory failed verification; delete it first to re-fetch: $Dest"
    exit 1
}

# Temp dir lives under the target parent so the final move stays on one volume.
# The system temp dir (usually C:) and the repo drive (D:) may differ; a cross-volume
# move degrades to copy+delete and can leave a half-published tree on interruption.
$TmpRoot = Join-Path (Split-Path -Parent $Dest) (".fetch-tmp-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $TmpRoot | Out-Null

try {
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
    & tar.exe -xzf $Archive -C $Extract
    if ($LASTEXITCODE -ne 0) { throw "tar.exe failed with exit $LASTEXITCODE" }

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

    Write-Host "==> Full verification against baseline manifest before publish"
    & $Check -Dir $SourceRoot
    if ($LASTEXITCODE -ne 0) { throw "Pinned source verification failed" }

    Write-Host "==> Publish to stable directory"
    Move-Item -LiteralPath $SourceRoot -Destination $Dest

    Write-Host "OK  vendor\upstream\pi-$PiVersion ($PiTag @ $PiCommit)"
}
finally {
    Remove-Item -LiteralPath $TmpRoot -Recurse -Force -ErrorAction SilentlyContinue
}
