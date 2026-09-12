#define MyAppName "CoKernel"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "CoKernel"
#define MyAppExeName "CoKernel.Desktop.exe"

[Setup]
AppId={{C838E2B0-57E3-46A2-B109-2D2369C884BE}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\CoKernel
DefaultGroupName=CoKernel
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist\installer
OutputBaseFilename=CoKernelSetup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#MyAppExeName}
SetupLogging=yes

[Dirs]
Name: "{app}\runtime\workspace"

[Files]
Source: "..\..\dist\desktop\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\.env.example"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "..\..\compose.yaml"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "..\..\*.cmd"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "..\..\*.ps1"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "..\..\*.sh"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "..\..\docker\*"; DestDir: "{app}\runtime\docker"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\scripts\*"; DestDir: "{app}\runtime\scripts"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\mcp-extension\*"; DestDir: "{app}\runtime\mcp-extension"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\CoKernel"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\CoKernel"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"; Flags: unchecked

[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\runtime\install-desktop.ps1"""; WorkingDir: "{app}\runtime"; Description: "Provision the CoKernel WSL/Linux GPU runtime"; Flags: postinstall skipifsilent
Filename: "{app}\{#MyAppExeName}"; Description: "Launch CoKernel Desktop"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    MsgBox('The CoKernel application will be removed. The dedicated CoKernel WSL distribution and its workspace are intentionally preserved. Use an explicit maintenance command if you later choose to delete that data.', mbInformation, MB_OK);
end;
