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
DisableDirPage=auto
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
english.DeleteData=Also delete settings and saved scans?
romanian.DeleteData=Ștergeți și setările și scanările salvate?
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

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{userappdata}\FastFileRecovery');
    { Never delete data when silent. }
    if (not UninstallSilent) and DirExists(DataDir) then
      if MsgBox(CustomMessage('DeleteData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(DataDir, True, True, True);
  end;
end;
