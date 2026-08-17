# Verify the pinned pi source tree matches the checked-in manifest byte-for-byte.
# Used by check-pins.ps1 and fetch-pi-source.ps1.
# Usage: check-pi-source-pin.ps1 [-Dir <path>]
#   Default dir is vendor\upstream\pi-0.84.2; fetch passes its temp extraction dir.
param([string]$Dir)

$ErrorActionPreference = "Stop"

$PiVersion = "0.84.2"
$PiTag     = "v$PiVersion"
$PiCommit  = "914cf1472e715297caa30db4b9535d534a9eb718"
$SourceSha256 = "65077457f18f9d3b0bc642870c5c19f41e38378e7f0ba4c3dd0962989e7d0036"
$SourceUrl = "https://codeload.github.com/earendil-works/pi/tar.gz/refs/tags/$PiTag"
$Root      = Split-Path -Parent $PSScriptRoot
if (-not $Dir) { $Dir = Join-Path $Root "vendor\upstream\pi-$PiVersion" }
$Marker    = Join-Path $Dir ".gpui-pi-source-pin"
$Manifest  = Join-Path $Root "pins\pi-$PiVersion.manifest"

$fail = $false
function Fail([string]$Message) {
    Write-Error "FAIL $Message" -ErrorAction Continue
    $script:fail = $true
}

if (-not (Test-Path -LiteralPath $Manifest -PathType Leaf)) {
    throw "Manifest baseline is missing: $Manifest"
}

if (-not (Test-Path -LiteralPath $Dir -PathType Container)) {
    Fail "pi source reference is not prepared; run .\scripts\fetch-pi-source.ps1"
    exit 1
}

if (Test-Path -LiteralPath (Join-Path $Dir ".git")) {
    Fail "stable source directory contains .git and may drift: $Dir"
} else {
    Write-Host "OK   pi source directory has no .git"
}

# Marker must be exactly the five pinned key=value lines (line-based exact match).
$ExpectedPairs = @(
    "version=$PiVersion"
    "tag=$PiTag"
    "commit=$PiCommit"
    "archive_sha256=$SourceSha256"
    "source=$SourceUrl"
)
if (Test-Path -LiteralPath $Marker -PathType Leaf) {
    # Trim CR so markers written by either bash or PowerShell validate identically.
    $MarkerLines = Get-Content -LiteralPath $Marker | ForEach-Object { $_.TrimEnd("`r") } |
                   Where-Object { $_ -ne "" }
    if ($MarkerLines.Count -ne 5) {
        Fail "marker line count is not 5 (got $($MarkerLines.Count)): $Marker"
    }
    foreach ($Expected in $ExpectedPairs) {
        if ($MarkerLines -notcontains $Expected) {
            Fail "marker is missing or mismatched: $Expected"
        }
    }
    Write-Host "OK   pi source marker (version/tag/commit/archive_sha256/source)"
} else {
    Fail "stable source directory is missing its pin marker: $Marker"
}

# Full content check: recompute SHA256 + size for every file except the marker and
# compare line-by-line against the baseline manifest.
function Get-SourceManifest([string]$SourceDir) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $items = New-Object System.Collections.Generic.List[object]
    foreach ($file in [System.IO.Directory]::EnumerateFiles($SourceDir, '*', 'AllDirectories')) {
        if ([System.IO.Path]::GetFileName($file) -eq '.gpui-pi-source-pin') { continue }
        $rel = $file.Substring($SourceDir.Length + 1).Replace('\', '/')
        $fs = [System.IO.File]::OpenRead($file)
        try {
            $hash = [BitConverter]::ToString($sha.ComputeHash($fs)).Replace('-', '').ToLower()
            $len = $fs.Length
        } finally { $fs.Dispose() }
        $items.Add([pscustomobject]@{ Path = $rel; Line = "$hash  $len  $rel" })
    }
    # Baseline is sorted by path in byte order (LC_ALL=C); Ordinal matches that for ASCII paths.
    $items.Sort([System.Comparison[object]]{ param($a, $b) [System.StringComparer]::Ordinal.Compare($a.Path, $b.Path) })
    return ,($items | ForEach-Object { $_.Line })
}

$Current = Get-SourceManifest $Dir
$Baseline = Get-Content -LiteralPath $Manifest | ForEach-Object { $_.TrimEnd("`r") }
if ($Current.Count -ne $Baseline.Count) {
    Fail "file count mismatch: current $($Current.Count) vs baseline $($Baseline.Count)"
} else {
    $mismatch = 0
    for ($i = 0; $i -lt $Current.Count; $i++) {
        if ($Current[$i] -ne $Baseline[$i]) {
            if ($mismatch -lt 20) {
                Write-Error "      baseline: $($Baseline[$i])" -ErrorAction Continue
                Write-Error "      current : $($Current[$i])" -ErrorAction Continue
            }
            $mismatch++
        }
    }
    if ($mismatch -eq 0) {
        Write-Host "OK   pi source content matches baseline manifest ($($Current.Count) files)"
    } else {
        Fail "pi source content differs from baseline manifest ($mismatch files)"
    }
}

if ($fail) { exit 1 } else { exit 0 }
