# Verify the pinned pi-web reference tree matches the checked-in manifest byte-for-byte.
# Used by check-pins.ps1 and fetch-pi-web.ps1.
# Usage: check-pi-web-pin.ps1 [-Dir <path>]
param([string]$Dir)

$ErrorActionPreference = "Stop"

$PiWebVersion = "0.8.9"
$PiWebTag     = "v$PiWebVersion"
$PiWebCommit  = "2a6e53710f6409e0cceb3de839a62f8cdf3ca3ca"
$PiWebSha256  = "9624948a2194e51d6d99208ce74dcd648f4886654d167fefd0afd84588d44883"
$PiWebUrl     = "https://codeload.github.com/agegr/pi-web/tar.gz/refs/tags/$PiWebTag"
$Root         = Split-Path -Parent $PSScriptRoot
if (-not $Dir) { $Dir = Join-Path $Root "vendor\upstream\pi-web-$PiWebVersion" }
$Marker       = Join-Path $Dir ".gpui-pi-web-source-pin"
$Manifest     = Join-Path $Root "pins\pi-web-$PiWebVersion.manifest"

$fail = $false
function Fail([string]$Message) {
    Write-Error "FAIL $Message" -ErrorAction Continue
    $script:fail = $true
}

if (-not (Test-Path -LiteralPath $Manifest -PathType Leaf)) {
    throw "Manifest baseline is missing: $Manifest"
}

if (-not (Test-Path -LiteralPath $Dir -PathType Container)) {
    Fail "pi-web reference is not prepared; run .\scripts\fetch-pi-web.ps1"
    exit 1
}

if (Test-Path -LiteralPath (Join-Path $Dir ".git")) {
    Fail "stable reference directory contains .git and may drift: $Dir"
} else {
    Write-Host "OK   pi-web source directory has no .git"
}

$ExpectedPairs = @(
    "version=$PiWebVersion"
    "tag=$PiWebTag"
    "commit=$PiWebCommit"
    "archive_sha256=$PiWebSha256"
    "source=$PiWebUrl"
)
if (Test-Path -LiteralPath $Marker -PathType Leaf) {
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
    Write-Host "OK   pi-web source marker (version/tag/commit/archive_sha256/source)"
} else {
    Fail "stable reference directory is missing its pin marker: $Marker"
}

function Get-SourceManifest([string]$SourceDir) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $items = New-Object System.Collections.Generic.List[object]
    foreach ($file in [System.IO.Directory]::EnumerateFiles($SourceDir, '*', 'AllDirectories')) {
        if ([System.IO.Path]::GetFileName($file) -eq '.gpui-pi-web-source-pin') { continue }
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
        Write-Host "OK   pi-web source content matches baseline manifest ($($Current.Count) files)"
    } else {
        Fail "pi-web source content differs from baseline manifest ($mismatch files)"
    }
}

if ($fail) { exit 1 } else { exit 0 }
