# BenShu Windows Build & Package Script
# Generates Lite (binary only) and Recommended (Tools + Bash) installers.

$VERSION = "0.3.5"
$BUILD_DIR = "target\release"
$BIN_DIR = "bin"
$DATA_DIR = "data"
$LLAMA_CPP_BUILD = 9592

Write-Host "--- 1. Building BenShu Core & Gateway ---" -ForegroundColor Cyan
cargo build --release -p benshu-gateway
cargo build --release -p benshu-panel

if (-not (Test-Path $BIN_DIR)) { New-Item -ItemType Directory -Path $BIN_DIR }

Write-Host "--- 2. Collecting Standalone Binaries ---" -ForegroundColor Cyan
# Downloading standalone tools for bundling (if not already present locally)
if (-not (Test-Path "$BIN_DIR\uv.exe")) {
    Write-Host "Downloading uv.exe..."
    Invoke-WebRequest -Uri "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-pc-windows-msvc.zip" -OutFile "$BIN_DIR\uv.zip"
    Expand-Archive -Path "$BIN_DIR\uv.zip" -DestinationPath "$BIN_DIR\uv_tmp" -Force
    Move-Item -Path "$BIN_DIR\uv_tmp\uv.exe" -Destination "$BIN_DIR\uv.exe" -Force
    Remove-Item -Path "$BIN_DIR\uv.zip", "$BIN_DIR\uv_tmp" -Recurse
}

if (-not (Test-Path "$BIN_DIR\pixi.exe")) {
    Write-Host "Downloading pixi.exe..."
    Invoke-WebRequest -Uri "https://github.com/prefix-dev/pixi/releases/latest/download/pixi-x86_64-pc-windows-msvc.exe" -OutFile "$BIN_DIR\pixi.exe"
}

if (-not (Test-Path "$BIN_DIR\bun.exe")) {
    Write-Host "Downloading bun.exe..."
    Invoke-WebRequest -Uri "https://github.com/oven-sh/bun/releases/download/bun-v1.2.4/bun-windows-x64.zip" -OutFile "$BIN_DIR\bun.zip"
    Expand-Archive -Path "$BIN_DIR\bun.zip" -DestinationPath "$BIN_DIR\bun_tmp" -Force
    Move-Item -Path "$BIN_DIR\bun_tmp\bun-windows-x64\bun.exe" -Destination "$BIN_DIR\bun.exe" -Force
    Remove-Item -Path "$BIN_DIR\bun.zip", "$BIN_DIR\bun_tmp" -Recurse
}

if (-not (Test-Path "$BIN_DIR\git-bash\bin\git.exe")) {
    Write-Host "Downloading portable Git (MinGit) for Bash support..."
    Invoke-WebRequest -Uri "https://github.com/git-for-windows/git/releases/download/v2.53.0.windows.1/MinGit-2.53.0-64-bit.zip" -OutFile "$BIN_DIR\mingit.zip"
    Expand-Archive -Path "$BIN_DIR\mingit.zip" -DestinationPath "$BIN_DIR\git-bash" -Force
    Remove-Item -Path "$BIN_DIR\mingit.zip"
}

if (-not (Test-Path "$BIN_DIR\mingw\bin\gcc.exe")) {
    Write-Host "Downloading portable GCC (w64devkit)..."
    Invoke-WebRequest -Uri "https://github.com/skeeto/w64devkit/releases/download/v1.21.0/w64devkit-1.21.0.zip" -OutFile "$BIN_DIR\mingw.zip"
    Expand-Archive -Path "$BIN_DIR\mingw.zip" -DestinationPath "$BIN_DIR\mingw_tmp" -Force
    # Move the contents of w64devkit folder out to bin/mingw
    Move-Item -Path "$BIN_DIR\mingw_tmp\w64devkit\*" -Destination "$BIN_DIR\mingw" -Force
    Remove-Item -Path "$BIN_DIR\mingw.zip", "$BIN_DIR\mingw_tmp" -Recurse
}

if (-not (Test-Path "$BIN_DIR\ffmpeg.exe")) {
    Write-Host "--- 2.5 Downloading Portable FFmpeg (Essentials) ---" -ForegroundColor Cyan
    # Using a reliable essential build zip
    $FFMPEG_URL = "https://github.com/GyanD/codexffmpeg/releases/download/7.1/ffmpeg-7.1-essentials_build.zip"
    Write-Host "Downloading FFmpeg from $FFMPEG_URL ..."
    Invoke-WebRequest -Uri $FFMPEG_URL -OutFile "$BIN_DIR\ffmpeg.zip"
    Expand-Archive -Path "$BIN_DIR\ffmpeg.zip" -DestinationPath "$BIN_DIR\ffmpeg_tmp" -Force
    
    # Extract only the necessary binaries to the bin root
    Get-ChildItem -Path "$BIN_DIR\ffmpeg_tmp\*\bin\*.exe" | ForEach-Object {
        Move-Item -Path $_.FullName -Destination "$BIN_DIR\$($_.Name)" -Force
    }
    
    Remove-Item -Path "$BIN_DIR\ffmpeg.zip", "$BIN_DIR\ffmpeg_tmp" -Recurse
    Write-Host "FFmpeg & FFprobe integrated successfully." -ForegroundColor Green
}

Write-Host "--- 3. Preparing Bundled llama.cpp Runtime (Recommended Version) ---" -ForegroundColor Cyan
& ".\scripts\windows\provision_llama_cpp_runtime.ps1" -Build $LLAMA_CPP_BUILD -RuntimeRoot "runtimes\llama.cpp"
if ($LASTEXITCODE -ne 0) {
    throw "Failed to provision bundled llama.cpp runtime."
}

Write-Host "--- 4. Preparing Pre-provisioned Bash Environment (Recommended Version) ---" -ForegroundColor Cyan
# In a real CI environment, we would run 'pixi install bash' here
# and copy the result to data/envs/bash for bundling.
if (-not (Test-Path "$DATA_DIR\envs\bash")) { New-Item -ItemType Directory -Path "$DATA_DIR\envs\bash" -Force }

Write-Host "--- 5. Generating Inno Setup Installer ---" -ForegroundColor Cyan
if (Get-Command "iscc" -ErrorAction SilentlyContinue) {
    iscc benshu_setup.iss
} else {
    Write-Warning "Inno Setup Compiler (iscc.exe) not found in PATH. Skipping installer generation."
    Write-Host "You can still use the collected files in $BUILD_DIR and $BIN_DIR manually."
}

Write-Host "Done! Setup file generated as benshu_setup.exe (if ISCC was available)." -ForegroundColor Green
