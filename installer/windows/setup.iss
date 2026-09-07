; FluxDown Windows Installer Script (Inno Setup)
; This script is used by GitHub Actions to build the installer.

#define MyAppName "FluxDown"
#define MyAppPublisher "FluxDown"
#define MyAppURL "https://github.com/user/x_down"
#define MyAppExeName "flux_down.exe"

; Version is passed from CI via /DMyAppVersion=x.y.z
#ifndef MyAppVersion
  #define MyAppVersion "1.0.0"
#endif

; Architecture is passed from CI via /DMyAppArch=x64 or /DMyAppArch=arm64
#ifndef MyAppArch
  #define MyAppArch "x64"
#endif

[Setup]
AppId={{B7E3F2A1-5C4D-4E8F-9A6B-1D2E3F4A5B6C}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..\..\build\installer
OutputBaseFilename=FluxDown-{#MyAppVersion}-windows-{#MyAppArch}-setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
#if MyAppArch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesInstallIn64BitMode=x64compatible
#endif
PrivilegesRequired=lowest
; ACL 受限场景（如安装目录由提权进程创建）允许用户改选管理员安装，
; 而不是在写文件时直接 access denied 死路。静默安装（自动更新 /SILENT）
; 不弹此对话框，仍按 lowest 执行。
PrivilegesRequiredOverridesAllowed=dialog
CloseApplications=force
SetupIconFile=..\..\windows\runner\resources\app_icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[CustomMessages]
english.OtherTasks=Other:
chinesesimplified.OtherTasks=其他：
english.FileAssociations=File associations:
chinesesimplified.FileAssociations=文件关联：
english.LaunchOnStartup=Launch at system startup
chinesesimplified.LaunchOnStartup=开机时自动启动
english.TorrentAssoc=Associate .torrent files with FluxDown
chinesesimplified.TorrentAssoc=将 .torrent 文件关联到 FluxDown

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "launchonstartup"; Description: "{cm:LaunchOnStartup}"; GroupDescription: "{cm:OtherTasks}"; Flags: unchecked
Name: "torrentassoc"; Description: "{cm:TorrentAssoc}"; GroupDescription: "{cm:FileAssociations}"; Flags: unchecked

[Files]
; Install all files from the Flutter build output
Source: "..\..\build\windows\{#MyAppArch}\runner\Release\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
; First install: create desktop icon only if user checks the task
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon
; Overlay/update install: always refresh the shortcut if it already exists on desktop
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Check: DesktopIconAlreadyExists

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent shellexec
Filename: "{app}\{#MyAppExeName}"; Flags: nowait skipifdoesntexist skipifnotsilent runasoriginaluser

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#MyAppName}"; ValueData: """{app}\{#MyAppExeName}"" --silentStart"; Flags: uninsdeletevalue; Tasks: launchonstartup

; .torrent file association
Root: HKCU; Subkey: "Software\Classes\.torrent"; ValueType: string; ValueData: "FluxDown.TorrentFile"; Flags: uninsdeletekey; Tasks: torrentassoc
Root: HKCU; Subkey: "Software\Classes\FluxDown.TorrentFile"; ValueType: string; ValueData: "BitTorrent File"; Flags: uninsdeletekey; Tasks: torrentassoc
Root: HKCU; Subkey: "Software\Classes\FluxDown.TorrentFile\DefaultIcon"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"",0"; Flags: uninsdeletekey; Tasks: torrentassoc
Root: HKCU; Subkey: "Software\Classes\FluxDown.TorrentFile\shell\open\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" ""%1"""; Flags: uninsdeletekey; Tasks: torrentassoc

[UninstallDelete]
; 删除 KvStore 落盘文件（含匿名统计设备 ID / 首装标记 / 窗口状态等本地偏好）。
; 语义：卸载后重装 = 生成新设备 ID = 统计为新安装；升级/覆盖安装不触发本节，ID 保留。
; 路径 = shared_preferences_windows：%APPDATA%\<CompanyName>\<ProductName>（Runner.rc 均为 FluxDown）。
Type: files; Name: "{userappdata}\FluxDown\FluxDown\shared_preferences.json"
Type: dirifempty; Name: "{userappdata}\FluxDown\FluxDown"
Type: dirifempty; Name: "{userappdata}\FluxDown"

; NMH manifest JSON files written at runtime by native/hub/src/nmh_registry.rs
; into the exe's own directory (never installed via [Files], so the standard
; uninstall never learns about them and leaves them on disk).
Type: files; Name: "{app}\com.fluxdown.nmh.json"
Type: files; Name: "{app}\com.fluxdown.nmh.firefox.json"

[Code]
function DesktopIconAlreadyExists: Boolean;
begin
  Result := FileExists(ExpandConstant('{autodesktop}\{#MyAppName}.lnk'));
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Result := '';
  { Force-kill flux_down.exe as a fallback in case Restart Manager fails }
  Exec('taskkill', '/f /im {#MyAppExeName}', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  { Upgrade installs must overwrite the previous uninstaller; a stray
    read-only attribute on it makes CreateFile fail with access denied.
    Clear the attribute up-front (no-op when the files do not exist). }
  Exec('attrib', '-r "' + ExpandConstant('{app}\unins000.exe') + '"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('attrib', '-r "' + ExpandConstant('{app}\unins000.dat') + '"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  { Small delay to ensure file locks are released }
  Sleep(500);
end;

{ Extract the quoted executable path from a `"<exe>" "%1"`-style
  shell\open\command value (the format written by
  native/hub/src/protocol_registry.rs::register and nmh_registry.rs). }
function ExtractQuotedExe(const Command: String): String;
var
  FirstQuote, SecondQuote: Integer;
begin
  Result := '';
  FirstQuote := Pos('"', Command);
  if FirstQuote = 0 then Exit;
  SecondQuote := Pos('"', Copy(Command, FirstQuote + 1, MaxInt));
  if SecondQuote = 0 then Exit;
  Result := Copy(Command, FirstQuote + 1, SecondQuote - 1);
end;

{ Remove a URL scheme handler (fluxdown:// / ed2k:// / magnet:) registered at runtime by
  native/hub/src/protocol_registry.rs. These keys live under
  HKCU\Software\Classes\<scheme> and are never declared in [Registry] — the
  standard uninstall never removes them, and Windows tries to relaunch the
  deleted exe whenever a matching link is opened. Only removes the key if it
  still points at this install's exe, so a handler since reclaimed by another
  app (e.g. eMule re-registering ed2k://) is left untouched. }
procedure RemoveProtocolHandler(const Scheme: String);
var
  Command, RegisteredExe, AppExe: String;
begin
  if not RegQueryStringValue(HKCU, 'Software\Classes\' + Scheme + '\shell\open\command', '', Command) then
    Exit;
  RegisteredExe := ExtractQuotedExe(Command);
  AppExe := ExpandConstant('{app}\{#MyAppExeName}');
  if (RegisteredExe <> '') and (CompareText(RegisteredExe, AppExe) = 0) then
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\' + Scheme);
end;

{ Remove the `.torrent` file association registered at runtime by
  native/hub/src/file_association.rs (toggled from the app's settings page,
  独立于 install-time 的 torrentassoc task — that task's [Registry] entries
  carry uninsdeletekey, but a runtime-written association is invisible to the
  uninstall log). Only removes the ProgID tree if its shell\open\command
  still points at this install's exe, and only removes the `.torrent`
  extension key if it still maps to our ProgID (mirrors the conservative
  ownership check in file_association.rs::disassociate). }
procedure RemoveTorrentAssociation;
var
  Command, RegisteredExe, AppExe, ProgId: String;
begin
  if not RegQueryStringValue(HKCU, 'Software\Classes\FluxDown.TorrentFile\shell\open\command', '', Command) then
    Exit;
  RegisteredExe := ExtractQuotedExe(Command);
  AppExe := ExpandConstant('{app}\{#MyAppExeName}');
  if (RegisteredExe = '') or (CompareText(RegisteredExe, AppExe) <> 0) then
    Exit;
  if RegQueryStringValue(HKCU, 'Software\Classes\.torrent', '', ProgId)
    and (CompareText(ProgId, 'FluxDown.TorrentFile') = 0) then
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\.torrent');
  RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\FluxDown.TorrentFile');
end;

{ Remove the autostart Run value written at runtime by the launch_at_startup
  plugin (lib/main.dart — value name "FluxDown", data `"<exe>" --silentStart`).
  The app migrates even the installer-written task entry to this runtime form
  on first launch (lib/main.dart, "legacy/installer autostart entry"
  migration), so after any app run the uninstall log no longer matches the
  value and uninsdeletevalue alone cannot be relied on. Only removes the
  value if it still points at this install's exe. }
procedure RemoveAutostartRunValue;
var
  Command, RegisteredExe, AppExe: String;
begin
  if not RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#MyAppName}', Command) then
    Exit;
  RegisteredExe := ExtractQuotedExe(Command);
  { Legacy entries may store the path unquoted; fall back to the raw value
    with any trailing arguments stripped. }
  if RegisteredExe = '' then
  begin
    RegisteredExe := Trim(Command);
    if Pos(' --', RegisteredExe) > 0 then
      RegisteredExe := Trim(Copy(RegisteredExe, 1, Pos(' --', RegisteredExe) - 1));
  end;
  AppExe := ExpandConstant('{app}\{#MyAppExeName}');
  if (RegisteredExe <> '') and (CompareText(RegisteredExe, AppExe) = 0) then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#MyAppName}');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    { Chrome/Edge/Firefox Native Messaging Host registrations written at
      runtime by native/hub/src/nmh_registry.rs. Never declared in the
      Registry section (the app writes them directly via winreg on every startup),
      so the standard uninstall never removes them. `com.fluxdown.nmh` is
      FluxDown-specific, safe to remove unconditionally. }
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Google\Chrome\NativeMessagingHosts\com.fluxdown.nmh');
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Microsoft\Edge\NativeMessagingHosts\com.fluxdown.nmh');
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Mozilla\NativeMessagingHosts\com.fluxdown.nmh');

    { fluxdown:// / ed2k:// / magnet: URL protocol handlers — same gap as above. }
    RemoveProtocolHandler('fluxdown');
    RemoveProtocolHandler('ed2k');
    RemoveProtocolHandler('magnet');

    { .torrent association + autostart Run value — runtime-written variants
      of the [Registry] task entries, invisible to the uninstall log. }
    RemoveTorrentAssociation;
    RemoveAutostartRunValue;
  end;
end;
