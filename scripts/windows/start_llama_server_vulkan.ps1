param(
    [Parameter(Mandatory = $true)]
    [string]$ServerExe,

    [int]$MinBuild = 9592,

    [Parameter(Mandatory = $true)]
    [string]$ModelPath,

    [string]$MmprojPath = "",
    [string]$MediaPath = "",
    [int]$Port = 8012,
    [int]$CtxSize = 8192,
    [int]$GpuLayers = 99,
    [int]$Threads = -1,
    [string]$ThreadsBatch = "",
    [int]$BatchSize = 2048,
    [int]$UbatchSize = 512,
    [int]$ParallelSlots = 1,
    [string]$CacheRam = "256",
    [string]$CtxCheckpoints = "0",
    [string]$FlashAttnMode = "auto",
    [string]$KvOffload = "true",
    [string]$Mmap = "true",
    [string]$Mlock = "false",
    [string]$CachePrompt = "false",
    [string]$ContBatching = "false",
    [string]$Warmup = "true",
    [string]$ContextShift = "false",
    [string]$Jinja = "true",
    [string]$RopeScaling = "",
    [string]$RopeScale = "",
    [string]$RopeFreqBase = "",
    [string]$RopeFreqScale = "",
    [string]$YarnOrigCtx = "",
    [string]$YarnExtFactor = "",
    [string]$YarnAttnFactor = "",
    [string]$YarnBetaSlow = "",
    [string]$YarnBetaFast = "",
    [string]$CacheTypeK = "",
    [string]$CacheTypeV = "",
    [string]$Device = "",
    [string]$SplitMode = "",
    [string]$TensorSplit = "",
    [string]$MainGpu = "",
    [string]$FitMode = "on",
    [string]$FitTarget = "",
    [string]$FitCtx = "",
    [string]$CpuMoe = "false",
    [string]$NCpuMoe = "",
    [string]$MmprojOffload = "true",
    [string]$ImageMinTokens = "",
    [string]$ImageMaxTokens = "",
    [string]$ReasoningMode = "auto",
    [string]$ReasoningFormat = "auto",
    [string]$ReasoningBudget = "",
    [string]$ReasoningBudgetMessage = "",
    [string]$SamplingTemperature = "0.8",
    [string]$SamplingTopK = "40",
    [string]$SamplingTopP = "0.95",
    [string]$SamplingMinP = "0.05",
    [string]$SamplingTypicalP = "1.0",
    [string]$SamplingRepeatPenalty = "1.0",
    [string]$SamplingPresencePenalty = "0.0",
    [string]$SamplingFrequencyPenalty = "0.0",
    [string]$SamplingMirostat = "0",
    [string]$SamplingMirostatEta = "0.1",
    [string]$SamplingMirostatTau = "5.0",
    [string]$Seed = "",
    [string]$BindHost = "127.0.0.1",
    [string]$Alias = "benshu-main-brain",
    [string]$ApiKey = "sk-local-llama-key",
    [int]$ReadyTimeoutSecs = 240,
    [string]$PidFile = "$env:TEMP\\benshu-llama-vulkan.pid",
    [string]$StdoutLogFile = "$env:TEMP\\benshu-llama-vulkan.out.log",
    [string]$StderrLogFile = "$env:TEMP\\benshu-llama-vulkan.err.log"
)

$ErrorActionPreference = "Stop"

function Get-LlamaCppBuild {
    param([string]$Path)
    $versionText = ""
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $versionText = (& $Path --version 2>&1 | Out-String)
    } catch {
        $versionText = "$versionText`n$($_.Exception.Message)"
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    if ($versionText -match "version:\s*(\d+)") {
        return [int]$Matches[1]
    }
    if ($versionText -match "build\s*:?\s*b(\d+)") {
        return [int]$Matches[1]
    }
    if ($versionText -match "\bb(\d{4,})\b") {
        return [int]$Matches[1]
    }
    throw "Unable to parse llama.cpp build from version output: $versionText"
}

function Get-ManagedLlamaServerProcesses {
    $serverName = [System.IO.Path]::GetFileName($ServerExe)
    $processes = @(Get-CimInstance Win32_Process | Where-Object {
            $_.Name -eq $serverName -and
            $_.CommandLine -like "*--port $Port*" -and
            $_.CommandLine -like "*$ModelPath*"
        })
    return $processes
}

function Get-BenShuManagedLlamaServerProcesses {
    $serverName = [System.IO.Path]::GetFileName($ServerExe)
    $processes = @(Get-CimInstance Win32_Process | Where-Object {
            $_.Name -eq $serverName -and
            (
                $_.CommandLine -like "*--alias $Alias*" -or
                $_.CommandLine -like "*--alias=`"$Alias`"*" -or
                $_.CommandLine -like "*benshu-main-brain*"
            )
        })
    return $processes
}

function Stop-ManagedLlamaServerProcesses {
    $processes = @(Get-ManagedLlamaServerProcesses)
    foreach ($process in $processes) {
        if ($process.ProcessId) {
            Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
            Write-Output "Stopped stale llama-server PID=$($process.ProcessId)"
        }
    }
    return $processes.Count
}

function Stop-BenShuManagedLlamaServerProcesses {
    $processes = @(Get-BenShuManagedLlamaServerProcesses)
    foreach ($process in $processes) {
        if ($process.ProcessId) {
            Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
            Write-Output "Stopped BenShu-managed stale llama-server PID=$($process.ProcessId)"
        }
    }
    return $processes.Count
}

function Get-PortListeners {
    return @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
}

function Stop-PortConflicts {
    $listeners = @(Get-PortListeners)
    foreach ($listener in $listeners) {
        if (-not $listener.OwningProcess) {
            continue
        }
        $pid = [int]$listener.OwningProcess
        if ($pid -eq $PID) {
            continue
        }
        $proc = Get-CimInstance Win32_Process -Filter "ProcessId=$pid" -ErrorAction SilentlyContinue
        if ($proc) {
            Write-Output "Stopping port $Port conflict PID=$pid Name=$($proc.Name)"
        } else {
            Write-Output "Stopping port $Port conflict PID=$pid"
        }
        Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue
    }
    return $listeners.Count
}

function Convert-ToBool {
    param(
        [Parameter(Mandatory = $true)]
        [AllowNull()]
        [object]$Value,

        [bool]$Default = $false
    )

    if ($Value -is [bool]) {
        return $Value
    }

    if ($null -eq $Value) {
        return $Default
    }

    $text = [string]::Concat($Value).Trim().ToLowerInvariant()
    switch ($text) {
        "1" { return $true }
        "true" { return $true }
        "yes" { return $true }
        "on" { return $true }
        "0" { return $false }
        "false" { return $false }
        "no" { return $false }
        "off" { return $false }
        "" { return $Default }
        default { throw "Invalid boolean value: $Value" }
    }
}

function Test-QwenModel {
    param([string]$Path)
    if (-not $Path) {
        return $false
    }
    return $Path.ToLowerInvariant().Contains("qwen")
}

function Test-ReasoningModeDisabled {
    param([string]$Value)
    $text = ([string]::Concat($Value)).Trim().ToLowerInvariant()
    return @("off", "false", "none", "disabled", "0") -contains $text
}

function Test-QwenReasoningFormatIncompatible {
    param([string]$Value)
    $text = ([string]::Concat($Value)).Trim().ToLowerInvariant()
    return @("", "false", "off", "none", "disabled", "0", "auto") -contains $text
}

$KvOffloadRaw = $KvOffload
$MmapRaw = $Mmap
$MlockRaw = $Mlock
$CachePromptRaw = $CachePrompt
$ContBatchingRaw = $ContBatching
$WarmupRaw = $Warmup
$ContextShiftRaw = $ContextShift
$JinjaRaw = $Jinja
$CpuMoeRaw = $CpuMoe
$MmprojOffloadRaw = $MmprojOffload

$KvOffloadBool = Convert-ToBool -Value $KvOffload -Default $true
$MmapBool = Convert-ToBool -Value $Mmap -Default $true
$MlockBool = Convert-ToBool -Value $Mlock -Default $false
$CachePromptBool = Convert-ToBool -Value $CachePrompt -Default $false
$ContBatchingBool = Convert-ToBool -Value $ContBatching -Default $false
$WarmupBool = Convert-ToBool -Value $Warmup -Default $true
$ContextShiftBool = Convert-ToBool -Value $ContextShift -Default $false
$JinjaBool = Convert-ToBool -Value $Jinja -Default $true
$CpuMoeBool = Convert-ToBool -Value $CpuMoe -Default $false
$MmprojOffloadBool = Convert-ToBool -Value $MmprojOffload -Default $true

if (-not (Test-Path -LiteralPath $ServerExe)) {
    throw "llama-server executable not found: $ServerExe"
}

$serverBuild = Get-LlamaCppBuild -Path $ServerExe
if ($serverBuild -lt $MinBuild) {
    throw "llama-server build b$serverBuild is older than required b$MinBuild. Update BenShu bundled llama.cpp before loading this GGUF model."
}
Write-Output "llama.cpp build b$serverBuild satisfies minimum b$MinBuild"

if (-not (Test-Path -LiteralPath $ModelPath)) {
    throw "Model path not found: $ModelPath"
}

if ($MmprojPath -and -not (Test-Path -LiteralPath $MmprojPath)) {
    throw "mmproj path not found: $MmprojPath"
}

if ($MediaPath -and -not (Test-Path -LiteralPath $MediaPath)) {
    throw "media path not found: $MediaPath"
}

$reasoningDisabled = Test-ReasoningModeDisabled -Value $ReasoningMode
if ($reasoningDisabled) {
    $adjustedReasoning = $false
    if ($ReasoningMode -ne "off") {
        $ReasoningMode = "off"
        $adjustedReasoning = $true
    }
    if ($ReasoningFormat -ne "none") {
        $ReasoningFormat = "none"
        $adjustedReasoning = $true
    }
    if ($ReasoningBudget) {
        $ReasoningBudget = ""
        $adjustedReasoning = $true
    }
    if ($ReasoningBudgetMessage) {
        $ReasoningBudgetMessage = ""
        $adjustedReasoning = $true
    }
    if ($adjustedReasoning) {
        Write-Output "Applied llama.cpp reasoning-off preset: reasoning=$ReasoningMode reasoning-format=$ReasoningFormat"
    }
} elseif (Test-QwenModel -Path $ModelPath) {
    $adjustedReasoning = $false
    if (Test-QwenReasoningFormatIncompatible -Value $ReasoningFormat) {
        $ReasoningFormat = "deepseek"
        $adjustedReasoning = $true
    }
    if ($adjustedReasoning) {
        Write-Output "Applied Qwen llama.cpp reasoning compatibility preset: reasoning=$ReasoningMode reasoning-format=$ReasoningFormat"
    }
}

$allManagedProcesses = @(Get-BenShuManagedLlamaServerProcesses)
if ($allManagedProcesses.Count -gt 0) {
    Write-Output "Found $($allManagedProcesses.Count) BenShu-managed llama-server process(es); cleaning up before restart."
    Stop-BenShuManagedLlamaServerProcesses | Out-Null
    Start-Sleep -Milliseconds 500
}

$matchingProcesses = @(Get-ManagedLlamaServerProcesses)
if (Test-Path -LiteralPath $PidFile) {
    $existingPid = (Get-Content -LiteralPath $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1).Trim()
    if ($existingPid -and $matchingProcesses.Count -eq 1) {
        $existing = Get-Process -Id $existingPid -ErrorAction SilentlyContinue
        if ($existing -and [int]$matchingProcesses[0].ProcessId -eq [int]$existingPid) {
            Write-Output "llama-server already running (PID=$existingPid)"
            Write-Output "URL=http://$BindHost`:$Port/v1"
            exit 0
        }
    }
}

if ($matchingProcesses.Count -gt 0) {
    Write-Output "Found $($matchingProcesses.Count) stale llama-server process(es) for port $Port; cleaning up before restart."
    Stop-ManagedLlamaServerProcesses | Out-Null
    Start-Sleep -Milliseconds 500
}

$portConflicts = @(Get-PortListeners)
if ($portConflicts.Count -gt 0) {
    Write-Output "Found $($portConflicts.Count) existing listener(s) on port $Port; cleaning up before restart."
    Stop-PortConflicts | Out-Null
    Start-Sleep -Milliseconds 500
}

if (Test-Path -LiteralPath $PidFile) {
    Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
}

New-Item -ItemType File -Path $StdoutLogFile -Force | Out-Null
New-Item -ItemType File -Path $StderrLogFile -Force | Out-Null

$arguments = @(
    "--host", $BindHost,
    "--port", $Port.ToString(),
    "-m", $ModelPath,
    "-c", $CtxSize.ToString(),
    "-ngl", $GpuLayers.ToString(),
    "-t", $Threads.ToString(),
    "-b", $BatchSize.ToString(),
    "-ub", $UbatchSize.ToString(),
    "--parallel", $ParallelSlots.ToString(),
    "--alias", $Alias,
    "--api-key", $ApiKey
)

if ($MmprojPath) {
    $arguments += @("--mmproj", $MmprojPath)
}

if ($ThreadsBatch) {
    $arguments += @("-tb", $ThreadsBatch)
}

if ($CacheRam) {
    $arguments += @("--cache-ram", $CacheRam)
}

if ($CtxCheckpoints) {
    $arguments += @("--ctx-checkpoints", $CtxCheckpoints)
}

if ($CachePromptBool) {
    $arguments += @("--cache-prompt")
} else {
    $arguments += @("--no-cache-prompt")
}

if ($ContBatchingBool) {
    $arguments += @("--cont-batching")
} else {
    $arguments += @("--no-cont-batching")
}

if ($WarmupBool) {
    $arguments += @("--warmup")
} else {
    $arguments += @("--no-warmup")
}

if ($ContextShiftBool) {
    $arguments += @("--context-shift")
} else {
    $arguments += @("--no-context-shift")
}

if ($JinjaBool) {
    $arguments += @("--jinja")
} else {
    $arguments += @("--no-jinja")
}

if ($FlashAttnMode) {
    $arguments += @("-fa", $FlashAttnMode)
}

if ($KvOffloadBool) {
    $arguments += @("--kv-offload")
} else {
    $arguments += @("--no-kv-offload")
}

if ($MmapBool) {
    $arguments += @("--mmap")
} else {
    $arguments += @("--no-mmap")
}

if ($MlockBool) {
    $arguments += @("--mlock")
}

if ($RopeScaling) {
    $arguments += @("--rope-scaling", $RopeScaling)
}

if ($RopeScale) {
    $arguments += @("--rope-scale", $RopeScale)
}

if ($RopeFreqBase) {
    $arguments += @("--rope-freq-base", $RopeFreqBase)
}

if ($RopeFreqScale) {
    $arguments += @("--rope-freq-scale", $RopeFreqScale)
}

if ($YarnOrigCtx) {
    $arguments += @("--yarn-orig-ctx", $YarnOrigCtx)
}

if ($YarnExtFactor) {
    $arguments += @("--yarn-ext-factor", $YarnExtFactor)
}

if ($YarnAttnFactor) {
    $arguments += @("--yarn-attn-factor", $YarnAttnFactor)
}

if ($YarnBetaSlow) {
    $arguments += @("--yarn-beta-slow", $YarnBetaSlow)
}

if ($YarnBetaFast) {
    $arguments += @("--yarn-beta-fast", $YarnBetaFast)
}

if ($CacheTypeK) {
    $arguments += @("-ctk", $CacheTypeK)
}

if ($CacheTypeV) {
    $arguments += @("-ctv", $CacheTypeV)
}

if ($Device) {
    $arguments += @("--device", $Device)
}

if ($SplitMode) {
    $arguments += @("-sm", $SplitMode)
}

if ($TensorSplit) {
    $arguments += @("-ts", $TensorSplit)
}

if ($MainGpu) {
    $arguments += @("-mg", $MainGpu)
}

if ($FitMode) {
    $arguments += @("-fit", $FitMode)
}

if ($FitTarget) {
    $arguments += @("-fitt", $FitTarget)
}

if ($FitCtx) {
    $arguments += @("-fitc", $FitCtx)
}

if ($CpuMoeBool) {
    $arguments += @("--cpu-moe")
}

if ($NCpuMoe) {
    $arguments += @("-ncmoe", $NCpuMoe)
}

if ($MmprojPath) {
    if ($MmprojOffloadBool) {
        $arguments += @("--mmproj-offload")
    } else {
        $arguments += @("--no-mmproj-offload")
    }
}

if ($ImageMinTokens) {
    $arguments += @("--image-min-tokens", $ImageMinTokens)
}

if ($ImageMaxTokens) {
    $arguments += @("--image-max-tokens", $ImageMaxTokens)
}

if ($ReasoningFormat) {
    $arguments += @("--reasoning-format", $ReasoningFormat)
}

if ($ReasoningMode) {
    $arguments += @("--reasoning", $ReasoningMode)
}

if ($ReasoningBudget) {
    $arguments += @("--reasoning-budget", $ReasoningBudget)
}

if ($ReasoningBudgetMessage) {
    $arguments += @("--reasoning-budget-message", $ReasoningBudgetMessage)
}

if ($SamplingTemperature) {
    $arguments += @("--temp", $SamplingTemperature)
}

if ($SamplingTopK) {
    $arguments += @("--top-k", $SamplingTopK)
}

if ($SamplingTopP) {
    $arguments += @("--top-p", $SamplingTopP)
}

if ($SamplingMinP) {
    $arguments += @("--min-p", $SamplingMinP)
}

if ($SamplingTypicalP) {
    $arguments += @("--typical", $SamplingTypicalP)
}

if ($SamplingRepeatPenalty) {
    $arguments += @("--repeat-penalty", $SamplingRepeatPenalty)
}

if ($SamplingPresencePenalty) {
    $arguments += @("--presence-penalty", $SamplingPresencePenalty)
}

if ($SamplingFrequencyPenalty) {
    $arguments += @("--frequency-penalty", $SamplingFrequencyPenalty)
}

if ($SamplingMirostat) {
    $arguments += @("--mirostat", $SamplingMirostat)
}

if ($SamplingMirostatEta) {
    $arguments += @("--mirostat-lr", $SamplingMirostatEta)
}

if ($SamplingMirostatTau) {
    $arguments += @("--mirostat-ent", $SamplingMirostatTau)
}

if ($Seed) {
    $arguments += @("-s", $Seed)
}

if ($MediaPath) {
    $arguments += @("--media-path", $MediaPath)
}

$argumentsPreview = ($arguments | ForEach-Object {
    if ($_ -match '\s') {
        '"' + $_ + '"'
    } else {
        $_
    }
}) -join ' '

Write-Output "ARGUMENTS=$argumentsPreview"

$proc = Start-Process `
    -FilePath $ServerExe `
    -ArgumentList $arguments `
    -WindowStyle Hidden `
    -RedirectStandardOutput $StdoutLogFile `
    -RedirectStandardError $StderrLogFile `
    -PassThru

Set-Content -LiteralPath $PidFile -Value $proc.Id
Start-Sleep -Milliseconds 500
$proc.Refresh()
if ($proc.HasExited) {
    $stderrTail = if (Test-Path -LiteralPath $StderrLogFile) {
        (Get-Content -LiteralPath $StderrLogFile -Tail 40 -ErrorAction SilentlyContinue) -join "`n"
    } else {
        ""
    }
    throw "llama-server exited immediately after launch on port $Port. STDERR tail:`n$stderrTail"
}

Write-Output "PID=$($proc.Id)"
Write-Output "URL=http://$BindHost`:$Port/v1"
Write-Output "STDOUT_LOG=$StdoutLogFile"
Write-Output "STDERR_LOG=$StderrLogFile"
Write-Output "KV_OFFLOAD_RAW=$KvOffloadRaw"
Write-Output "MMAP_RAW=$MmapRaw"
Write-Output "MLOCK_RAW=$MlockRaw"
Write-Output "CACHE_PROMPT_RAW=$CachePromptRaw"
Write-Output "CONT_BATCHING_RAW=$ContBatchingRaw"
Write-Output "WARMUP_RAW=$WarmupRaw"
Write-Output "CONTEXT_SHIFT_RAW=$ContextShiftRaw"
Write-Output "JINJA_RAW=$JinjaRaw"
Write-Output "CPU_MOE_RAW=$CpuMoeRaw"
Write-Output "MMPROJ_OFFLOAD_RAW=$MmprojOffloadRaw"
Write-Output "KV_OFFLOAD=$KvOffloadBool"
Write-Output "MMAP=$MmapBool"
Write-Output "MLOCK=$MlockBool"
Write-Output "CACHE_PROMPT=$CachePromptBool"
Write-Output "CONT_BATCHING=$ContBatchingBool"
Write-Output "WARMUP=$WarmupBool"
Write-Output "CONTEXT_SHIFT=$ContextShiftBool"
Write-Output "JINJA=$JinjaBool"
Write-Output "CPU_MOE=$CpuMoeBool"
Write-Output "MMPROJ_OFFLOAD=$MmprojOffloadBool"

function Stop-StartedLlamaServer {
    param([System.Diagnostics.Process]$Process)
    try {
        if ($Process -and -not $Process.HasExited) {
            Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        }
    } catch {
    }
}

function Test-LlamaTextProbe {
    param(
        [string]$BaseUrl,
        [string]$ModelAlias,
        [string]$Key
    )

    $body = @{
        model = $ModelAlias
        messages = @(@{
            role = "user"
            content = "Print exactly: BENSHU_READY"
        })
        temperature = 0.0
        max_tokens = 16
    } | ConvertTo-Json -Depth 8

    try {
        $headers = @{ Authorization = "Bearer $Key" }
        $response = Invoke-RestMethod `
            -Uri "$BaseUrl/chat/completions" `
            -Method Post `
            -Headers $headers `
            -ContentType "application/json" `
            -Body $body `
            -TimeoutSec 10
        $content = [string]$response.choices[0].message.content
        if ($content.Contains("BENSHU_READY")) {
            return $true
        }
        $sample = ($content -replace "\s+", " ").Trim()
        if ($sample.Length -gt 240) {
            $sample = $sample.Substring(0, 240)
        }
        throw "expected BENSHU_READY, got: '$sample'"
    } catch {
        throw "llama-server text probe failed: $($_.Exception.Message)"
    }
}

try {
    $probeReady = $false
    $lastProbeError = $null
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds([Math]::Max(1, $ReadyTimeoutSecs))
    while ([DateTimeOffset]::UtcNow -lt $deadline) {
        try {
            Test-LlamaTextProbe `
                -BaseUrl "http://$BindHost`:$Port/v1" `
                -ModelAlias $Alias `
                -Key $ApiKey | Out-Null
            $probeReady = $true
            break
        } catch {
            $lastProbeError = $_.Exception.Message
            Start-Sleep -Seconds 1
        }
    }
    if (-not $probeReady) {
        throw $lastProbeError
    }
    Write-Output "TEXT_PROBE=BENSHU_READY"
} catch {
    Stop-StartedLlamaServer -Process $proc
    throw $_
}

exit 0
