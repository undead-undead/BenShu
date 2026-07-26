param(
    [int]$Build = 9592,
    [string]$RuntimeRoot = "runtimes\llama.cpp",
    [string]$SourceDir = "",
    [string]$ArchivePath = "",
    [string]$DownloadUrl = "",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

function Get-LlamaCppBuild {
    param([string]$ServerExe)

    if (-not (Test-Path $ServerExe)) {
        throw "llama-server executable not found: $ServerExe"
    }

    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $output = & $ServerExe --version 2>&1
    } finally {
        $ErrorActionPreference = $previousPreference
    }

    $versionText = ($output | Out-String)
    foreach ($line in $versionText -split "`r?`n") {
        if ($line -match "version:\s*(\d+)") {
            return [int]$Matches[1]
        }
        if ($line -match "build\s*[:=]\s*b?(\d+)") {
            return [int]$Matches[1]
        }
        if ($line -match "\bb(\d{3,})\b") {
            return [int]$Matches[1]
        }
    }
    throw "Unable to parse llama.cpp build from version output: $versionText"
}

function Copy-RuntimeDirectory {
    param(
        [string]$From,
        [string]$To
    )

    if (-not (Test-Path $From)) {
        throw "SourceDir not found: $From"
    }
    if (-not (Test-Path (Join-Path $From "llama-server.exe"))) {
        throw "SourceDir does not contain llama-server.exe: $From"
    }

    if (Test-Path $To) {
        Remove-Item $To -Recurse -Force
    }
    New-Item -ItemType Directory -Path $To -Force | Out-Null
    Copy-Item -Path (Join-Path $From "*") -Destination $To -Recurse -Force
}

function Expand-RuntimeArchive {
    param(
        [string]$Archive,
        [string]$To
    )

    if (-not (Test-Path $Archive)) {
        throw "ArchivePath not found: $Archive"
    }

    $temp = Join-Path ([System.IO.Path]::GetTempPath()) ("benshu-llama-cpp-" + [System.Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $temp -Force | Out-Null
    try {
        Expand-Archive -Path $Archive -DestinationPath $temp -Force
        $server = Get-ChildItem -Path $temp -Filter "llama-server.exe" -Recurse | Select-Object -First 1
        if ($null -eq $server) {
            throw "Archive does not contain llama-server.exe: $Archive"
        }
        Copy-RuntimeDirectory -From $server.Directory.FullName -To $To
    } finally {
        if (Test-Path $temp) {
            Remove-Item $temp -Recurse -Force
        }
    }
}

function Find-LocalRuntimeSource {
    param([int]$Build)

    $candidates = @()
    if (-not [string]::IsNullOrWhiteSpace($env:BENSHU_WINDOWS_LLAMA_CPP_DIR)) {
        $candidates += $env:BENSHU_WINDOWS_LLAMA_CPP_DIR
    }
    if (-not [string]::IsNullOrWhiteSpace($env:LLAMA_CPP_DIR)) {
        $candidates += $env:LLAMA_CPP_DIR
    }
    $candidates += @(
        "D:\llama.cpp\b$Build",
        "C:\llama.cpp\b$Build",
        "D:\llama.cpp",
        "C:\llama.cpp"
    )

    foreach ($candidate in $candidates) {
        if ([string]::IsNullOrWhiteSpace($candidate)) {
            continue
        }
        $server = Join-Path $candidate "llama-server.exe"
        if (Test-Path $server) {
            return $candidate
        }
    }
    return $null
}

$dest = Join-Path $RuntimeRoot ("b$Build")
$serverExe = Join-Path $dest "llama-server.exe"

if ((Test-Path $serverExe) -and (-not $Force)) {
    $existingBuild = Get-LlamaCppBuild -ServerExe $serverExe
    if ($existingBuild -ge $Build) {
        Write-Host "Bundled llama.cpp runtime already present: $serverExe (b$existingBuild)" -ForegroundColor Green
        exit 0
    }
    Write-Warning "Existing bundled llama.cpp build b$existingBuild is older than required b$Build; replacing it."
}

New-Item -ItemType Directory -Path $RuntimeRoot -Force | Out-Null

if (-not [string]::IsNullOrWhiteSpace($SourceDir)) {
    Copy-RuntimeDirectory -From $SourceDir -To $dest
} elseif (-not [string]::IsNullOrWhiteSpace($ArchivePath)) {
    Expand-RuntimeArchive -Archive $ArchivePath -To $dest
} else {
    $local = Find-LocalRuntimeSource -Build $Build
    if ($null -ne $local) {
        Write-Host "Using local llama.cpp runtime source: $local" -ForegroundColor Cyan
        Copy-RuntimeDirectory -From $local -To $dest
    } else {
        if ([string]::IsNullOrWhiteSpace($DownloadUrl)) {
            $DownloadUrl = "https://github.com/ggml-org/llama.cpp/releases/download/b$Build/llama-b$Build-bin-win-vulkan-x64.zip"
        }
        $archive = Join-Path $RuntimeRoot ("llama-b$Build-bin-win-vulkan-x64.zip")
        Write-Host "Downloading llama.cpp runtime b$Build from $DownloadUrl" -ForegroundColor Cyan
        Invoke-WebRequest -Uri $DownloadUrl -OutFile $archive
        Expand-RuntimeArchive -Archive $archive -To $dest
    }
}

$resolvedBuild = Get-LlamaCppBuild -ServerExe $serverExe
if ($resolvedBuild -lt $Build) {
    throw "Bundled llama.cpp build b$resolvedBuild is older than required b$Build."
}

Write-Host "Bundled llama.cpp runtime ready: $serverExe (b$resolvedBuild)" -ForegroundColor Green
