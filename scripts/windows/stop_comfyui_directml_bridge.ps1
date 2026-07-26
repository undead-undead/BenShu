param(
    [string]$PidFileComfy = "$env:TEMP\\benshu-comfyui.pid",
    [string]$PidFileBridge = "$env:TEMP\\benshu-comfyui-bridge.pid"
)

$ErrorActionPreference = "Stop"

function Stop-ByPidFile {
    param([string]$PidFile)

    if (-not (Test-Path -LiteralPath $PidFile)) {
        return
    }

    $pid = (Get-Content -LiteralPath $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1).Trim()
    if ($pid) {
        $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue
        if ($proc) {
            Stop-Process -Id $pid -Force
            Write-Output "Stopped PID=$pid"
        }
    }

    Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
}

Stop-ByPidFile -PidFile $PidFileBridge
Stop-ByPidFile -PidFile $PidFileComfy
