# BenShu Update Script for Windows
# This script is called by the gateway to update itself.

$repoUrl = "https://github.com/USER/benshu/releases/latest/download/benshu-gw.exe"
$destPath = "benshu-gw.exe"
$tempPath = "benshu-gw-new.exe"

Write-Host "[BenShu] Starting Windows Update..." -ForegroundColor Blue

# 1. Download new version
Write-Host "[BenShu] Downloading latest executable..." -ForegroundColor Blue
try {
    Invoke-WebRequest -Uri $repoUrl -OutFile $tempPath
} catch {
    Write-Error "Failed to download update: $_"
    exit 1
}

# 2. Create a swap script that runs after this process exits
$swapScript = @"
Start-Sleep -Seconds 2
Remove-Item "$destPath" -Force
Rename-Item "$tempPath" "$destPath"
Start-Process "$destPath" -ArgumentList "web"
Remove-Item "`$PSCommandPath"
"@

$swapScriptPath = "swap-update.ps1"
$swapScript | Out-File -FilePath $swapScriptPath -Encoding utf8

Write-Host "[OK] Download complete. BenShu will restart in 5 seconds to apply the update." -ForegroundColor Green
Write-Host "[INFO] Please close the application if it does not restart automatically." -ForegroundColor Yellow

# 3. Trigger the swap script in a new process and exit
Start-Process powershell -ArgumentList "-WindowStyle Hidden", "-ExecutionPolicy Bypass", "-File", "$swapScriptPath"

# The gateway should exit now.
# We return success so the panel sees the "Commenced" message.
exit 0
