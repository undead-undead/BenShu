param(
    [Parameter(Mandatory = $true)]
    [string]$CommandPath,

    [string]$Arguments = "",
    [string]$WorkingDirectory = "",
    [string]$PidFile = "$env:TEMP\\benshu-image-bridge.pid",
    [string]$StdoutLogFile = "$env:TEMP\\benshu-image-bridge.out.log",
    [string]$StderrLogFile = "$env:TEMP\\benshu-image-bridge.err.log",
    [string]$HealthUrl = "",
    [int]$HealthTimeoutSeconds = 60
)

$ErrorActionPreference = "Stop"
$scriptDir = Split-Path -Parent $PSCommandPath

& (Join-Path $scriptDir "stop_image_bridge_service.ps1") -PidFile $PidFile | Out-Host

& (Join-Path $scriptDir "start_image_bridge_service.ps1") `
    -CommandPath $CommandPath `
    -Arguments $Arguments `
    -WorkingDirectory $WorkingDirectory `
    -PidFile $PidFile `
    -StdoutLogFile $StdoutLogFile `
    -StderrLogFile $StderrLogFile `
    -HealthUrl $HealthUrl `
    -HealthTimeoutSeconds $HealthTimeoutSeconds
