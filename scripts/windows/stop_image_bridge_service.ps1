param(
    [string]$PidFile = "$env:TEMP\\benshu-image-bridge.pid"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $PidFile)) {
    Write-Output "No image bridge PID file found."
    exit 0
}

$pid = (Get-Content -LiteralPath $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1).Trim()
if ($pid) {
    $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue
    if ($proc) {
        Stop-Process -Id $pid -Force
        Write-Output "Stopped PID=$pid"
    } else {
        Write-Output "PID file existed but process was already gone."
    }
}

Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
