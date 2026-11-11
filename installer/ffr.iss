; Fast File Recovery — Inno Setup script. One file installs the app the first time and updates it later.
; Build: iscc /DAppVersion=X.Y.Z /DSourceExe=path\to\fast-file-recovery.exe installer\ffr.iss
; This file is UTF-8 with BOM.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\fast-file-recovery.exe"
#endif

#define AppName "Fast File Recovery"
#define AppExe "fast-file-recovery.exe"
; Marks the install folder; the app keeps its settings, saved scans and logs next to it
; (crates/ffr-app/src/install).
#define Marker ".ffr-install"

[Setup]
; AppId never changes: installing over an existing copy upgrades it in place.
AppId={{DF0DFA64-AFDA-4083-80B4-6577536FD656}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Akulav
AppPublisherURL=https://github.com/Akulav/fast-file-recovery
AppSupportURL=https://github.com/Akulav/fast-file-recovery/issues
AppUpdatesURL=https://github.com/Akulav/fast-file-recovery/releases
AppCopyright=Made by Akulav
VersionInfoVersion={#AppVersion}
VersionInfoCompany=Akulav
VersionInfoDescription=Fast File Recovery Setup
VersionInfoProductName={#AppName}
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\FastFileRecovery
DisableProgramGroupPage=yes
; Ask for the folder on the first install only; updates reuse the previous one.
DisableDirPage=auto
UsePreviousAppDir=yes
DirExistsWarning=no
DefaultGroupName={#AppName}
OutputBaseFilename=FastFileRecovery-setup
OutputDir=..\dist
SetupIconFile=..\assets\icons\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
WizardImageFile=wizard-large.bmp
WizardSmallImageFile=wizard-small.bmp
CloseApplications=force
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2/ultra64
SolidCompression=yes
LicenseFile=..\LICENSE

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "romanian"; MessagesFile: "Romanian.isl"

[CustomMessages]
english.DeleteData=Also delete settings, saved scans and logs?%n%nChoose No to keep them for a future reinstall.
romanian.DeleteData=Ștergeți și setările, scanările salvate și jurnalele?%n%nAlegeți Nu pentru a le păstra pentru o reinstalare.
english.DataNote=Fast File Recovery and everything it saves (settings, saved scans, logs) will be stored in this folder.
romanian.DataNote=Fast File Recovery și tot ce salvează (setări, scanări salvate, jurnale) vor fi păstrate în acest dosar.
english.LaunchApp=Launch Fast File Recovery
romanian.LaunchApp=Pornește Fast File Recovery

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Recover deleted files — Made by Akulav"
Name: "{userdesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
; Interactive install: normal "Launch" checkbox.
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchApp}"; Flags: nowait postinstall skipifsilent shellexec runasoriginaluser
; Silent update (/UPDATE): relaunch the app afterwards.
Filename: "{app}\{#AppExe}"; Flags: nowait shellexec runasoriginaluser; Check: IsUpdate

[UninstallDelete]
Type: files; Name: "{app}\{#Marker}"
; The downloaded update setup (in-app updates keep it in the app's own folder).
Type: filesandordirs; Name: "{app}\update"

[Code]
function IsUpdate(): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/UPDATE') = 0 then
      Result := True;
end;

procedure InitializeWizard();
begin
  WizardForm.SelectDirLabel.Caption := WizardForm.SelectDirLabel.Caption + #13#10 + CustomMessage('DataNote');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    SaveStringToFile(ExpandConstant('{app}\{#Marker}'), '{#AppVersion}', False);
end;

function HasAppData(Dir: String): Boolean;
begin
  Result := FileExists(Dir + '\settings.toml') or DirExists(Dir + '\sessions') or DirExists(Dir + '\logs');
end;

{ Only what the app itself creates, never anything else the user keeps in that folder. }
procedure DeleteAppData(Dir: String);
begin
  DelTree(Dir + '\sessions', True, True, True);
  DelTree(Dir + '\logs', True, True, True);
  DelTree(Dir + '\update', True, True, True);
  DeleteFile(Dir + '\settings.toml');
  RemoveDir(Dir);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  AppDir, OldDir: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    AppDir := ExpandConstant('{app}');
    { Versions before 0.1.3 kept their data here. }
    OldDir := ExpandConstant('{userappdata}\FastFileRecovery');
    { Never delete data when silent. }
    if (not UninstallSilent) and (HasAppData(AppDir) or DirExists(OldDir)) then
      if MsgBox(CustomMessage('DeleteData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      begin
        DeleteAppData(AppDir);
        DelTree(OldDir, True, True, True);
      end;
    RemoveDir(AppDir);
  end;
end;
