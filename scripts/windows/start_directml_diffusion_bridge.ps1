param(
    [Parameter(Mandatory = $true)]
    [string]$PythonExe,

    [Parameter(Mandatory = $true)]
    [string]$ModelDir,

    [string]$ModelAlias = "local-image-model",
    [string]$ListenHost = "0.0.0.0",
    [int]$Port = 8022,
    [int]$NumSteps = 4,
    [double]$GuidanceScale = 0.0,
    [string]$DType = "float16",
    [string]$PidFile = "$env:TEMP\\benshu-directml-diffusion.pid",
    [string]$StdoutLogFile = "$env:TEMP\\benshu-directml-diffusion.out.log",
    [string]$StderrLogFile = "$env:TEMP\\benshu-directml-diffusion.err.log"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $PythonExe)) {
    throw "Python executable not found: $PythonExe"
}

if (-not (Test-Path -LiteralPath $ModelDir)) {
    throw "Model directory not found: $ModelDir"
}

$scriptPath = Join-Path (Split-Path -Parent $PSCommandPath) "directml_diffusers_image_service.py"
if (-not (Test-Path -LiteralPath $scriptPath)) {
    throw "Service script not found: $scriptPath"
}

if (Test-Path -LiteralPath $PidFile) {
    $existingPid = (Get-Content -LiteralPath $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1).Trim()
    if ($existingPid) {
        $existing = Get-Process -Id $existingPid -ErrorAction SilentlyContinue
        if ($existing) {
            Write-Output "directml diffusion service already running (PID=$existingPid)"
            Write-Output "URL=http://$ListenHost`:$Port/v1"
            exit 0
        }
    }
    Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
}

$argList = @(
    $scriptPath,
    "--model-dir", $ModelDir,
    "--listen-host", $ListenHost,
    "--listen-port", $Port.ToString(),
    "--model-name", $ModelAlias,
    "--steps", $NumSteps.ToString(),
    "--guidance-scale", $GuidanceScale.ToString([System.Globalization.CultureInfo]::InvariantCulture),
    "--dtype", $DType
)

$proc = Start-Process `
    -FilePath $PythonExe `
    -ArgumentList $argList `
    -WorkingDirectory (Split-Path -Parent $scriptPath) `
    -WindowStyle Hidden `
    -RedirectStandardOutput $StdoutLogFile `
    -RedirectStandardError $StderrLogFile `
    -PassThru

Set-Content -LiteralPath $PidFile -Value $proc.Id

$healthUrl = "http://127.0.0.1:$Port/health"
$deadline = (Get-Date).AddSeconds(180)
while ((Get-Date) -lt $deadline) {
    try {
        $resp = Invoke-WebRequest -UseBasicParsing -Uri $healthUrl -TimeoutSec 3
        if ($resp.StatusCode -eq 200) {
            Write-Output "PID=$($proc.Id)"
            Write-Output "URL=http://$ListenHost`:$Port/v1"
            Write-Output "HEALTH_URL=$healthUrl"
            Write-Output "STDOUT_LOG=$StdoutLogFile"
            Write-Output "STDERR_LOG=$StderrLogFile"
            exit 0
        }
    } catch {
    }
    Start-Sleep -Seconds 2
}

throw "DirectML diffusion service did not become healthy within 180 seconds."
