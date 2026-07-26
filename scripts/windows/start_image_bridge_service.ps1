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

if (-not (Test-Path -LiteralPath $CommandPath)) {
    throw "Image bridge command not found: $CommandPath"
}

if (Test-Path -LiteralPath $PidFile) {
    $existingPid = (Get-Content -LiteralPath $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1).Trim()
    if ($existingPid) {
        $existing = Get-Process -Id $existingPid -ErrorAction SilentlyContinue
        if ($existing) {
            Write-Output "image-bridge already running (PID=$existingPid)"
            if ($HealthUrl) {
                Write-Output "HEALTH_URL=$HealthUrl"
            }
            exit 0
        }
    }
    Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
}

$startParams = @{
    FilePath = $CommandPath
    WindowStyle = "Hidden"
    RedirectStandardOutput = $StdoutLogFile
    RedirectStandardError = $StderrLogFile
    PassThru = $true
}

if ($Arguments) {
    $startParams.ArgumentList = $Arguments
}

if ($WorkingDirectory) {
    if (-not (Test-Path -LiteralPath $WorkingDirectory)) {
        throw "Working directory not found: $WorkingDirectory"
    }
    $startParams.WorkingDirectory = $WorkingDirectory
}

$proc = Start-Process @startParams
Set-Content -LiteralPath $PidFile -Value $proc.Id

Write-Output "PID=$($proc.Id)"
Write-Output "STDOUT_LOG=$StdoutLogFile"
Write-Output "STDERR_LOG=$StderrLogFile"

if (-not $HealthUrl) {
    exit 0
}

$deadline = (Get-Date).AddSeconds($HealthTimeoutSeconds)
while ((Get-Date) -lt $deadline) {
    try {
        $resp = Invoke-WebRequest -UseBasicParsing -Uri $HealthUrl -TimeoutSec 3
        if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 500) {
            Write-Output "HEALTH_URL=$HealthUrl"
            exit 0
        }
    } catch {
    }
    Start-Sleep -Seconds 1
}

throw "Image bridge service did not become healthy within $HealthTimeoutSeconds seconds: $HealthUrl"
