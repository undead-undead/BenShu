param(
    [string]$ServiceExe = "",
    [string]$PythonExe = "",

    [Parameter(Mandatory = $true)]
    [string]$SourceModelDir,

    [Parameter(Mandatory = $true)]
    [string]$OnnxModelDir,

    [string]$ExportPythonExe = "",
    [string]$ModelAlias = "local-image-model",
    [string]$ListenHost = "0.0.0.0",
    [int]$Port = 8022,
    [int]$NumSteps = 4,
    [double]$GuidanceScale = 0.0,
    [string]$NegativePrompt = "blurry, low quality, distorted, bad anatomy, deformed",
    [int]$DeviceId = 0,
    [string]$PidFile = "$env:TEMP\\benshu-onnx-directml-image.pid",
    [string]$StdoutLogFile = "$env:TEMP\\benshu-onnx-directml-image.out.log",
    [string]$StderrLogFile = "$env:TEMP\\benshu-onnx-directml-image.err.log"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $SourceModelDir)) {
    throw "Source model directory not found: $SourceModelDir"
}

$exportScript = Join-Path (Split-Path -Parent $PSCommandPath) "export_onnx_diffusion_model.py"
$scriptPath = Join-Path (Split-Path -Parent $PSCommandPath) "onnx_directml_image_service.py"
if (-not (Test-Path -LiteralPath $exportScript)) {
    throw "Export script not found: $exportScript"
}
if (-not (Test-Path -LiteralPath $scriptPath)) {
    throw "Service script not found: $scriptPath"
}

if ($ServiceExe -and -not (Test-Path -LiteralPath $ServiceExe)) {
    throw "Rust image service executable not found: $ServiceExe"
}

function Test-OnnxBundleReady {
    param([string]$PathValue)

    if (-not (Test-Path -LiteralPath $PathValue)) {
        return $false
    }

    $modelIndex = Join-Path $PathValue "model_index.json"
    if (-not (Test-Path -LiteralPath $modelIndex)) {
        return $false
    }

    return [bool](Get-ChildItem -LiteralPath $PathValue -Recurse -Filter *.onnx -ErrorAction SilentlyContinue | Select-Object -First 1)
}

if (-not (Test-OnnxBundleReady -PathValue $OnnxModelDir)) {
    if (-not $PythonExe) {
        throw "Python executable is required when ONNX bundle export is needed."
    }

    if (-not (Test-Path -LiteralPath $PythonExe)) {
        throw "Python executable not found: $PythonExe"
    }

    if (-not $ExportPythonExe) {
        $ExportPythonExe = $PythonExe
    }

    if (-not (Test-Path -LiteralPath $ExportPythonExe)) {
        throw "Export python executable not found: $ExportPythonExe"
    }

    New-Item -ItemType Directory -Path $OnnxModelDir -Force | Out-Null
    & $ExportPythonExe $exportScript --source-model-dir $SourceModelDir --output-dir $OnnxModelDir
}

if (Test-Path -LiteralPath $PidFile) {
    $existingPid = (Get-Content -LiteralPath $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1).Trim()
    if ($existingPid) {
        $existing = Get-Process -Id $existingPid -ErrorAction SilentlyContinue
        if ($existing) {
            Write-Output "onnx directml image service already running (PID=$existingPid)"
            Write-Output "URL=http://$ListenHost`:$Port/v1"
            exit 0
        }
    }
    Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
}

$env:BENSHU_ONNX_IMAGE_SOURCE_MODEL_DIR = $SourceModelDir
$env:BENSHU_ONNX_IMAGE_MODEL_DIR = $OnnxModelDir
$env:BENSHU_ONNX_IMAGE_HOST = $ListenHost
$env:BENSHU_ONNX_IMAGE_PORT = $Port.ToString()
$env:BENSHU_ONNX_IMAGE_MODEL_NAME = $ModelAlias
$env:BENSHU_ONNX_IMAGE_STEPS = $NumSteps.ToString()
$env:BENSHU_ONNX_IMAGE_GUIDANCE_SCALE = $GuidanceScale.ToString([System.Globalization.CultureInfo]::InvariantCulture)
$env:BENSHU_ONNX_IMAGE_NEGATIVE_PROMPT = $NegativePrompt
$env:BENSHU_ONNX_IMAGE_DEVICE_ID = $DeviceId.ToString()
$env:BENSHU_ONNX_DIFFUSION_SOURCE_MODEL_DIR = $SourceModelDir
$env:BENSHU_ONNX_DIFFUSION_MODEL_DIR = $OnnxModelDir
$env:BENSHU_ONNX_DIFFUSION_HOST = $ListenHost
$env:BENSHU_ONNX_DIFFUSION_PORT = $Port.ToString()
$env:BENSHU_ONNX_DIFFUSION_MODEL_NAME = $ModelAlias
$env:BENSHU_ONNX_DIFFUSION_STEPS = $NumSteps.ToString()
$env:BENSHU_ONNX_DIFFUSION_GUIDANCE_SCALE = $GuidanceScale.ToString([System.Globalization.CultureInfo]::InvariantCulture)
$env:BENSHU_ONNX_DIFFUSION_NEGATIVE_PROMPT = $NegativePrompt
$env:BENSHU_ONNX_DIFFUSION_DEVICE_ID = $DeviceId.ToString()

$serviceProc = if ($ServiceExe) {
    Start-Process `
        -FilePath $ServiceExe `
        -WorkingDirectory (Split-Path -Parent $ServiceExe) `
        -WindowStyle Hidden `
        -RedirectStandardOutput $StdoutLogFile `
        -RedirectStandardError $StderrLogFile `
        -PassThru
} else {
    if (-not $PythonExe) {
        throw "Python executable is required when Rust image service executable is not provided."
    }

    if (-not (Test-Path -LiteralPath $PythonExe)) {
        throw "Python executable not found: $PythonExe"
    }

    Start-Process `
        -FilePath $PythonExe `
        -ArgumentList @($scriptPath) `
        -WorkingDirectory (Split-Path -Parent $scriptPath) `
        -WindowStyle Hidden `
        -RedirectStandardOutput $StdoutLogFile `
        -RedirectStandardError $StderrLogFile `
        -PassThru
}

Set-Content -LiteralPath $PidFile -Value $serviceProc.Id

$healthUrl = "http://127.0.0.1:$Port/health"
$deadline = (Get-Date).AddSeconds(900)
while ((Get-Date) -lt $deadline) {
    try {
        $resp = Invoke-WebRequest -UseBasicParsing -Uri $healthUrl -TimeoutSec 5
        if ($resp.StatusCode -eq 200) {
            Write-Output "PID=$($serviceProc.Id)"
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

throw "ONNX DirectML image service did not become healthy within 900 seconds."
