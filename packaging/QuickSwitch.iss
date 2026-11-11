; QuickSwitch per-user installer (plan §10.1 step 4–5).
;
; - Installs to %LOCALAPPDATA%\Programs\QuickSwitch without admin rights.
; - "Start with Windows" is an unchecked, opt-in task.
; - Uninstall removes the app files and the app-owned HKCU Run value.
;
; Build with scripts\package.ps1, or directly:
;   ISCC.exe /DAppVersion=0.2.0 /DSourceExe=..\target\release\QuickSwitch.exe packaging\QuickSwitch.iss

#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\QuickSwitch.exe"
#endif

#define AppName "QuickSwitch"
#define AppExe "QuickSwitch.exe"
#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"

[Setup]
AppId={{6B0E8C1A-4F43-4B8E-9A55-2F3C9D1E7A21}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppName}
VersionInfoVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\dist
OutputBaseFilename=QuickSwitch-{#AppVersion}-setup
SetupIconFile=..\resources\quickswitch.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; The app is closed with "QuickSwitch.exe --exit" in [Code] below.
CloseApplications=no
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startup"; Description: "Start QuickSwitch when I sign in to Windows"; GroupDescription: "Startup:"; Flags: unchecked
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
; The user README (not the developer README.md).
Source: "README.txt"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Search and switch windows, Explorer tabs and folders (Ctrl+Win+K)"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Same value the tray menu's "Start with Windows" writes; only when the task is ticked.
Root: HKCU; Subkey: "{#RunKey}"; ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"" --background"; Tasks: startup

[Run]
Filename: "{app}\{#AppExe}"; Parameters: "--background"; Description: "Start QuickSwitch now (it runs in the notification area)"; Flags: nowait postinstall skipifsilent

[Code]
procedure ExitRunningQuickSwitch(Exe: String);
var
  ResultCode: Integer;
begin
  if FileExists(Exe) then
    Exec(Exe, '--exit', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  // Upgrading: ask a running copy to exit so its exe can be replaced.
  ExitRunningQuickSwitch(ExpandConstant('{app}\{#AppExe}'));
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    ExitRunningQuickSwitch(ExpandConstant('{app}\{#AppExe}'));
  if CurUninstallStep = usPostUninstall then
    // Remove the app-owned startup value, whether set here or from the tray menu.
    RegDeleteValue(HKEY_CURRENT_USER, '{#RunKey}', '{#AppName}');
end;
