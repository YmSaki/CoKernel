#ifndef Stage
  #error Stage must point to the assembled release directory
#endif
#ifndef Version
  #define Version "1.0.0-alpha.0-candidate.8"
#endif
#define Bin "versions\\" + Version
[Setup]
AppId={{CA3B0C50-8C42-4524-9E09-C918DD90F5B1}
AppName=CoKernel
AppVersion={#Version}
DefaultDirName={localappdata}\Programs\CoKernel
PrivilegesRequired=lowest
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
MinVersion=10.0.22000
OutputBaseFilename=CoKernelSetup
OutputDir=output
Compression=lzma2
SolidCompression=yes
DisableProgramGroupPage=yes
CloseApplications=no
UninstallDisplayIcon={app}\{#Bin}\cokernel-desktop.exe
[Files]
Source: "{#Stage}\*"; DestDir: "{app}\{#Bin}"; Flags: recursesubdirs createallsubdirs ignoreversion
Source: "setup.ps1"; DestDir: "{app}\{#Bin}"
Source: "lifecycle.ps1"; DestDir: "{app}\{#Bin}"
Source: "runtime-maintenance.sh"; DestDir: "{app}\{#Bin}"
Source: "setup.ps1"; Flags: dontcopy
Source: "lifecycle.ps1"; Flags: dontcopy
Source: "runtime-maintenance.sh"; Flags: dontcopy
Source: "{#Stage}\cokernel-host.exe"; Flags: dontcopy
[Icons]
Name: "{autoprograms}\CoKernel"; Filename: "{app}\{#Bin}\cokernel-desktop.exe"
Name: "{autoprograms}\Uninstall CoKernel"; Filename: "{uninstallexe}"
[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "CoKernelHost"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\RunOnce"; ValueName: "CoKernelSetup"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\RunOnce"; ValueName: "CoKernelV1Setup"; Flags: uninsdeletevalue
[Code]
var MaintenanceApproved: Boolean;
function RunMaintenance(Script, Mode: String; var Code: Integer): Boolean;
begin
  Result := Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -ExecutionPolicy Bypass -File "' + Script + '" -Mode ' + Mode +
    ' -AppRoot "' + ExpandConstant('{app}') + '" -MaintenanceAccepted',
    '', SW_SHOW, ewWaitUntilTerminated, Code);
end;
function PrepareToInstall(var NeedsRestart: Boolean): String;
var Code: Integer;
begin
  Result := '';
  if WizardSilent then begin
    Result := 'Interactive installation is required to confirm Session termination.';
    exit;
  end;
  if not MaintenanceApproved then
    MaintenanceApproved := MsgBox('Install or update CoKernel? Save edited notebooks first. Running computations will stop and variables held in memory will be lost. Saved notebooks, packages and connection settings will be preserved.', mbConfirmation, MB_YESNO) = IDYES;
  if not MaintenanceApproved then begin Result := 'Installation deferred. No application files changed.'; exit; end;
  ExtractTemporaryFile('setup.ps1');
  ExtractTemporaryFile('lifecycle.ps1');
  ExtractTemporaryFile('runtime-maintenance.sh');
  ExtractTemporaryFile('cokernel-host.exe');
  if not RunMaintenance(ExpandConstant('{tmp}\setup.ps1'), 'Prepare', Code) or (Code <> 0) then
    Result := 'Preparation failed. Existing data was retained. See CoKernel setup.log; retry after resolving the reported error.';
end;
procedure CurStepChanged(CurStep: TSetupStep);
var Code: Integer;
begin
  if CurStep = ssPostInstall then begin
    if not RunMaintenance(ExpandConstant('{app}\{#Bin}\setup.ps1'), 'Install', Code) then
      RaiseException('Could not launch CoKernel runtime setup.');
    if Code = 3010 then
      MsgBox('Restart Windows. CoKernel setup will resume after sign-in.', mbInformation, MB_OK)
    else if Code <> 0 then
      RaiseException('CoKernel runtime setup failed. Installation is incomplete. Run this installer again; preserved data and setup.log remain available.');
  end;
end;
function InitializeUninstall(): Boolean;
var Choice, Code: Integer; Mode: String;
begin
  Result := False;
  if UninstallSilent then exit;
  Choice := MsgBox('Remove ALL CoKernel data as well?' + #13#10 +
    'Yes: permanently delete managed WSL, notebooks, packages, caches and saved credentials.' + #13#10 +
    'No: remove the application and keep all compute data for reinstall.' + #13#10 +
    'Cancel: do nothing.' + #13#10 +
    'Both removal modes stop running computations. Save edited notebooks first.', mbConfirmation, MB_YESNOCANCEL);
  if Choice = IDCANCEL then exit;
  Mode := 'Preserve';
  if Choice = IDYES then begin
    if MsgBox('Permanently erase all CoKernel notebooks and compute data? This cannot be undone.', mbConfirmation, MB_YESNO) <> IDYES then exit;
    Mode := 'Remove';
  end;
  Result := RunMaintenance(ExpandConstant('{app}\{#Bin}\setup.ps1'), Mode, Code) and (Code = 0);
  if not Result then MsgBox('Removal failed. Application files have been kept. See setup.log before retrying.', mbError, MB_OK);
end;
