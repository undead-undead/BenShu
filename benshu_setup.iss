; BenShu Windows Setup Script (Inno Setup)
; Generates a professional installer with Lite and Full (offline-ready) versions.

#define MyAppName "BenShu"
#define MyAppVersion "0.3.5"
#define MyAppPublisher "BenShu Team"
#define MyAppURL "https://benshu.com"
#define MyAppExeName "BenShu.exe"

[Setup]
AppId={C6D21822-7717-4B2B-B3A9-F7B4A0E1F203}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={drive:C}\{#MyAppName}
DisableProgramGroupPage=yes
OutputDir=.
OutputBaseFilename=benshu_setup
Compression=lzma/max
SolidCompression=yes
WizardStyle=modern
; Allow the user to choose any drive (e.g., D:\BenShu)
AllowUNCPath=yes
DefaultGroupName={#MyAppName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimp"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Types]
Name: "recommended"; Description: "Recommended Version (Managers + Bash included)"
Name: "lite"; Description: "Lite Version (Smallest download, requires internet for managers)"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

[Components]
Name: "main"; Description: "Main Application (BenShu Core)"; Types: lite recommended custom; Flags: fixed
Name: "runtime"; Description: "Bundled local llama.cpp Runtime (recommended for GGUF/Qwen models)"; Types: recommended custom
Name: "tools"; Description: "Environment Managers (uv, pixi) & Bash"; Types: recommended custom

[Files]
; Unified Binary (Panel + Embedded Engine)
Source: "target\release\benshu-panel.exe"; Dest: "{app}\{#MyAppExeName}"; Flags: ignoreversion; Components: main

; BenShu-managed Windows runtime control scripts
Source: "scripts\windows\start_llama_server_vulkan.ps1"; DestDir: "{app}\scripts\windows"; Flags: ignoreversion; Components: main
Source: "scripts\windows\restart_llama_server_vulkan.ps1"; DestDir: "{app}\scripts\windows"; Flags: ignoreversion; Components: main
Source: "scripts\windows\stop_llama_server_vulkan.ps1"; DestDir: "{app}\scripts\windows"; Flags: ignoreversion; Components: main

; Bundled llama.cpp runtime. The gateway discovers this through {app}\runtimes\llama.cpp\bNNNN.
Source: "runtimes\llama.cpp\*"; DestDir: "{app}\runtimes\llama.cpp"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: runtime

; GPU Acceleration & Runtime DLLs (Bundled for out-of-the-box performance)
Source: "bin\vulkan-1.dll"; DestDir: "{app}"; Flags: ignoreversion; Components: main; Check: IsWin64
Source: "bin\llama.dll"; DestDir: "{app}"; Flags: ignoreversion; Components: main
Source: "bin\cudart64_*.dll"; DestDir: "{app}"; Flags: ignoreversion; Components: main; Check: HasNvidiaGpu


; Local Binaries (Bundled in Recommended Version)
Source: "bin\uv.exe"; DestDir: "{app}\data\bin"; Flags: ignoreversion; Components: tools
Source: "bin\pixi.exe"; DestDir: "{app}\data\bin"; Flags: ignoreversion; Components: tools
Source: "bin\mingw\*"; DestDir: "{app}\data\bin\mingw"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: tools

; Pre-provisioned Bash Environment (Recommended Version)
Source: "data\envs\bash\*"; DestDir: "{app}\data\envs\bash"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: tools

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

// Logic to ensure the 'data' center is initialized correctly
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    // Create the 'data' folder explicitly to ensure the gateway picks it up as the root
    CreateDir(ExpandConstant('{app}\data'));
    // Write a marker file so the app knows it was installed via the official setup
    SaveStringToFile(ExpandConstant('{app}\data\installed_via_setup.txt'), 'version=' + '{#MyAppVersion}', False);
  end;
end;
