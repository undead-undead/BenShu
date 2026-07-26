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
$scriptDir = Split-Path -Parent $PSCommandPath

& (Join-Path $scriptDir "stop_onnx_directml_image_bridge.ps1") -PidFile $PidFile | Out-Host

& (Join-Path $scriptDir "start_onnx_directml_image_bridge.ps1") `
    -ServiceExe $ServiceExe `
    -PythonExe $PythonExe `
    -SourceModelDir $SourceModelDir `
    -OnnxModelDir $OnnxModelDir `
    -ExportPythonExe $ExportPythonExe `
    -ModelAlias $ModelAlias `
    -ListenHost $ListenHost `
    -Port $Port `
    -NumSteps $NumSteps `
    -GuidanceScale $GuidanceScale `
    -NegativePrompt $NegativePrompt `
    -DeviceId $DeviceId `
    -PidFile $PidFile `
    -StdoutLogFile $StdoutLogFile `
    -StderrLogFile $StderrLogFile
