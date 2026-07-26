param(
    [string]$PidFile = "$env:TEMP\\benshu-onnx-directml-image.pid"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $PidFile)) {
    Write-Output "No ONNX DirectML image service pid file found."
    exit 0
}

$pid = (Get-Content -LiteralPath $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1).Trim()
if ($pid) {
    Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue
}

Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
Write-Output "ONNX DirectML image service stopped."
