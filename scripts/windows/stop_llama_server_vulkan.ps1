param(
    [string]$PidFile = "$env:TEMP\\benshu-llama-vulkan.pid",
    [int]$Port = 8012,
    [string]$Alias = "benshu-main-brain"
)

$ErrorActionPreference = "Stop"

$stopped = $false

$portProcesses = @(Get-CimInstance Win32_Process | Where-Object {
        $_.Name -eq "llama-server.exe" -and (
            $_.CommandLine -like "*--port $Port*" -or
            $_.CommandLine -like "*--alias $Alias*" -or
            $_.CommandLine -like "*--alias=`"$Alias`"*" -or
            $_.CommandLine -like "*benshu-main-brain*"
        )
    })

foreach ($process in $portProcesses) {
    if ($process.ProcessId) {
        Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
        Write-Output "Stopped BenShu-managed PID=$($process.ProcessId)"
        $stopped = $true
    }
}

if (-not (Test-Path -LiteralPath $PidFile)) {
    if (-not $stopped) {
        Write-Output "No PID file present."
    }
    exit 0
}

$serverPid = (Get-Content -LiteralPath $PidFile | Select-Object -First 1).Trim()
if ($serverPid) {
    $proc = Get-Process -Id $serverPid -ErrorAction SilentlyContinue
    if ($proc) {
        Stop-Process -Id $serverPid -Force
        Write-Output "Stopped PID=$serverPid"
        $stopped = $true
    } else {
        Write-Output "Process $serverPid already exited."
    }
}

Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
