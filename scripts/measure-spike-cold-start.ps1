param(
    [string]$Executable = (Join-Path $PSScriptRoot "..\target\release\spike.exe"),
    [int]$Runs = 5,
    [int]$TimeoutMs = 10000
)

$ErrorActionPreference = "Stop"
$ThresholdMs = 1500

# This is an automation proxy, not an exact first-frame measurement. A run is
# considered interactive when MainWindowHandle is non-zero and Responding is true.
$Executable = [System.IO.Path]::GetFullPath($Executable)
if (-not (Test-Path -LiteralPath $Executable)) {
    throw "Spike executable not found: $Executable. Build it with cargo build -p gpui-pi --bin spike --release --locked --offline."
}
if ($Runs -ne 5) {
    throw "R1 requires exactly 5 cold-start runs."
}

$measurements = @()
for ($run = 1; $run -le $Runs; $run++) {
    $process = $null
    try {
        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        $process = Start-Process -FilePath $Executable -PassThru
        $interactive = $false

        while ($stopwatch.ElapsedMilliseconds -lt $TimeoutMs) {
            Start-Sleep -Milliseconds 10
            $process.Refresh()
            if ($process.HasExited) {
                throw "Run $run exited before a responsive main window appeared."
            }
            if (($process.MainWindowHandle -ne [IntPtr]::Zero) -and $process.Responding) {
                $interactive = $true
                break
            }
        }

        $stopwatch.Stop()
        if (-not $interactive) {
            throw "Run $run timed out after $TimeoutMs ms."
        }

        $elapsed = [int]$stopwatch.ElapsedMilliseconds
        $measurements += $elapsed
        Write-Host ("Proxy run {0}: {1} ms" -f $run, $elapsed)
    }
    finally {
        if (($null -ne $process) -and (-not $process.HasExited)) {
            [void]$process.CloseMainWindow()
            if (-not $process.WaitForExit(2000)) {
                $process.Kill()
                $process.WaitForExit()
            }
            $process.Dispose()
        }
    }
}

$sorted = @($measurements | Sort-Object)
$median = $sorted[[int][Math]::Floor($sorted.Count / 2)]
Write-Host ("Proxy median: {0} ms (fixed threshold: < {1} ms)" -f $median, $ThresholdMs)
Write-Host "Proxy only: MainWindowHandle != 0 and Responding; not exact first-frame timing."
Write-Host "Do not use this result alone to mark the R1 cold-start gate complete."

if ($median -lt $ThresholdMs) {
    Write-Host "PROXY PASS"
    exit 0
}

Write-Host "PROXY FAIL"
exit 1
