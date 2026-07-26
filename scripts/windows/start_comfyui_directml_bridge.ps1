param(
    [Parameter(Mandatory = $true)]
    [string]$ComfyUiRoot,

    [Parameter(Mandatory = $true)]
    [string]$CheckpointName,

    [string]$PythonExe = "",
    [int]$ComfyUiPort = 8188,
    [int]$BridgePort = 8022,
    [string]$BridgeHost = "127.0.0.1",
    [string]$ModelAlias = "local-image-model",
    [string]$ComfyUiArgs = "--listen 127.0.0.1 --port 8188 --directml",
    [string]$PidFileComfy = "$env:TEMP\\benshu-comfyui.pid",
    [string]$PidFileBridge = "$env:TEMP\\benshu-comfyui-bridge.pid",
    [string]$StdoutComfy = "$env:TEMP\\benshu-comfyui.out.log",
    [string]$StderrComfy = "$env:TEMP\\benshu-comfyui.err.log",
    [string]$StdoutBridge = "$env:TEMP\\benshu-comfyui-bridge.out.log",
    [string]$StderrBridge = "$env:TEMP\\benshu-comfyui-bridge.err.log"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $ComfyUiRoot)) {
    throw "ComfyUI root not found: $ComfyUiRoot"
}

$mainPy = Join-Path $ComfyUiRoot "main.py"
if (-not (Test-Path -LiteralPath $mainPy)) {
    throw "ComfyUI main.py not found under: $ComfyUiRoot"
}

if (-not $PythonExe) {
    $embedded = Join-Path $ComfyUiRoot "python_embeded\\python.exe"
    if (Test-Path -LiteralPath $embedded) {
        $PythonExe = $embedded
    } else {
        throw "Python executable not provided, and ComfyUI portable embedded python was not found."
    }
}

if (-not (Test-Path -LiteralPath $PythonExe)) {
    throw "Python executable not found: $PythonExe"
}

$bridgeScript = Join-Path (Split-Path -Parent $PSCommandPath) "comfyui_openai_image_bridge.py"
if (-not (Test-Path -LiteralPath $bridgeScript)) {
    throw "Bridge script not found: $bridgeScript"
}

function Start-DetachedProcess {
    param(
        [string]$FilePath,
        [string]$ArgumentList,
        [string]$WorkingDirectory,
        [string]$PidFile,
        [string]$StdoutLog,
        [string]$StderrLog,
        [hashtable]$EnvironmentTable = @{}
    )

    if (Test-Path -LiteralPath $PidFile) {
        $existingPid = (Get-Content -LiteralPath $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1).Trim()
        if ($existingPid) {
            $existing = Get-Process -Id $existingPid -ErrorAction SilentlyContinue
            if ($existing) {
                return $existingPid
            }
        }
        Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
    }

    $proc = Start-Process `
        -FilePath $FilePath `
        -ArgumentList $ArgumentList `
        -WorkingDirectory $WorkingDirectory `
        -WindowStyle Hidden `
        -RedirectStandardOutput $StdoutLog `
        -RedirectStandardError $StderrLog `
        -PassThru `
        -Environment $EnvironmentTable

    Set-Content -LiteralPath $PidFile -Value $proc.Id
    return $proc.Id
}

$comfyPid = Start-DetachedProcess `
    -FilePath $PythonExe `
    -ArgumentList "`"$mainPy`" $ComfyUiArgs" `
    -WorkingDirectory $ComfyUiRoot `
    -PidFile $PidFileComfy `
    -StdoutLog $StdoutComfy `
    -StderrLog $StderrComfy

$comfyBaseUrl = "http://127.0.0.1:$ComfyUiPort"
$comfyHealth = "$comfyBaseUrl/system_stats"
$deadline = (Get-Date).AddSeconds(90)
while ((Get-Date) -lt $deadline) {
    try {
        $resp = Invoke-WebRequest -UseBasicParsing -Uri $comfyHealth -TimeoutSec 3
        if ($resp.StatusCode -eq 200) {
            break
        }
    } catch {
    }
    Start-Sleep -Seconds 1
}

$bridgeEnv = @{
    "BENSHU_COMFYUI_BASE_URL" = $comfyBaseUrl
    "BENSHU_COMFYUI_CHECKPOINT" = $CheckpointName
    "BENSHU_IMAGE_BRIDGE_HOST" = $BridgeHost
    "BENSHU_IMAGE_BRIDGE_PORT" = $BridgePort.ToString()
    "BENSHU_IMAGE_BRIDGE_MODEL" = $ModelAlias
}

$bridgePid = Start-DetachedProcess `
    -FilePath $PythonExe `
    -ArgumentList "`"$bridgeScript`"" `
    -WorkingDirectory $ComfyUiRoot `
    -PidFile $PidFileBridge `
    -StdoutLog $StdoutBridge `
    -StderrLog $StderrBridge `
    -EnvironmentTable $bridgeEnv

$bridgeHealth = "http://$BridgeHost`:$BridgePort/health"
$deadline = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $deadline) {
    try {
        $resp = Invoke-WebRequest -UseBasicParsing -Uri $bridgeHealth -TimeoutSec 3
        if ($resp.StatusCode -eq 200) {
            Write-Output "COMFYUI_PID=$comfyPid"
            Write-Output "BRIDGE_PID=$bridgePid"
            Write-Output "BRIDGE_URL=http://$BridgeHost`:$BridgePort/v1"
            Write-Output "HEALTH_URL=$bridgeHealth"
            exit 0
        }
    } catch {
    }
    Start-Sleep -Seconds 1
}

throw "ComfyUI bridge did not become healthy within 60 seconds."
