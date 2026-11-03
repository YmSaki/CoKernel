#ifndef Stage
  #error Stage must point to the assembled release directory
#endif
#ifndef Version
  #define Version "1.0.0-alpha.0"
#endif
[Setup]
AppId={{CA3B0C50-8C42-4524-9E09-C918DD90F5B1}
AppName=CoKernel
AppVersion={#Version}
DefaultDirName={localappdata}\Programs\CoKernel
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
OutputBaseFilename=CoKernelSetup
OutputDir=output
Compression=lzma2
SolidCompression=yes
DisableProgramGroupPage=yes
CloseApplications=no
UninstallDisplayIcon={app}\cokernel-desktop.exe
[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion
Source: "setup.ps1"; DestDir: "{app}"
[Icons]
Name: "{autoprograms}\CoKernel"; Filename: "{app}\cokernel-desktop.exe"
Name: "{autoprograms}\CoKernel Setup"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\setup.ps1"""
[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\setup.ps1"""; Description: "Prepare GPU runtime and open CoKernel"; Flags: postinstall skipifsilent
[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "CoKernelHost"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\RunOnce"; ValueName: "CoKernelV1Setup"; Flags: uninsdeletevalue
[Code]
function InitializeSetup(): Boolean;
begin
  Result := True;
  if FileExists(ExpandConstant('{localappdata}\CoKernelV1\runtime\host-wsl.json')) then begin
    MsgBox('An installed runtime already exists. This first-install package cannot migrate it. Existing notebooks and sessions are preserved.', mbError, MB_OK);
    Result := False;
  end;
end;
function InitializeUninstall(): Boolean;
begin
  Result := MsgBox('Remove the Windows application? GPU sessions must be stopped and CoKernel closed first. The WSL distribution, notebooks and saved credentials will be preserved.', mbConfirmation, MB_YESNO) = IDYES;
end;
