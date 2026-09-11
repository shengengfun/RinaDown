; RinaDown Windows Installer Script (Inno Setup)
; This script is used by GitHub Actions to build the installer.

#define MyAppName "RinaDown"
#define MyAppPublisher "RinaDown"
#define MyAppURL "https://github.com/shengengfun/RinaDown"
#define MyAppExeName "rina_down.exe"

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
; 始终显示「选择安装位置」向导页。默认值 auto 会在检测到同一 AppId 的既有安装
; （或 /RESTARTAPPLICATIONS 的覆盖安装）时直接跳过该页，用户看到的现象就是
; 「安装包不能自定义安装位置」。静默安装不受影响（本来也不显示页，用默认目录）。
DisableDirPage=no
; 绝不沿用旧安装记录的目录。2026-09 项目由 FluxDown 更名为 RinaDown，旧记录里
; 是 …\Programs\FluxDown；沿用会把新版本继续装进那只旧文件夹（改名等于没改）。
UsePreviousAppDir=no
OutputDir=..\..\build\installer
OutputBaseFilename=RinaDown-{#MyAppVersion}-windows-{#MyAppArch}-setup
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
; 向导右上角小图 = 应用 logo（由 scripts/gen_icons.ts 一并生成，与应用图标同源）。
; 用 24-bit BMP：PNG/JPEG 向导图需要 Inno Setup 6.3+，BMP 全版本通吃。
WizardSmallImageFile=wizard_small.bmp
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
english.TorrentAssoc=Associate .torrent files with RinaDown
chinesesimplified.TorrentAssoc=将 .torrent 文件关联到 RinaDown

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
Root: HKCU; Subkey: "Software\Classes\.torrent"; ValueType: string; ValueData: "RinaDown.TorrentFile"; Flags: uninsdeletekey; Tasks: torrentassoc
Root: HKCU; Subkey: "Software\Classes\RinaDown.TorrentFile"; ValueType: string; ValueData: "BitTorrent File"; Flags: uninsdeletekey; Tasks: torrentassoc
Root: HKCU; Subkey: "Software\Classes\RinaDown.TorrentFile\DefaultIcon"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"",0"; Flags: uninsdeletekey; Tasks: torrentassoc
Root: HKCU; Subkey: "Software\Classes\RinaDown.TorrentFile\shell\open\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" ""%1"""; Flags: uninsdeletekey; Tasks: torrentassoc

[UninstallDelete]
; 删除 KvStore 落盘文件（含匿名统计设备 ID / 首装标记 / 窗口状态等本地偏好）。
; 语义：卸载后重装 = 生成新设备 ID = 统计为新安装；升级/覆盖安装不触发本节，ID 保留。
; 路径 = shared_preferences_windows：%APPDATA%\<CompanyName>\<ProductName>（Runner.rc 均为 RinaDown）。
Type: files; Name: "{userappdata}\RinaDown\RinaDown\shared_preferences.json"
Type: dirifempty; Name: "{userappdata}\RinaDown\RinaDown"
Type: dirifempty; Name: "{userappdata}\RinaDown"

; NMH manifest JSON files written at runtime by native/hub/src/nmh_registry.rs
; into the exe's own directory (never installed via [Files], so the standard
; uninstall never learns about them and leaves them on disk).
Type: files; Name: "{app}\com.rinadown.nmh.json"
Type: files; Name: "{app}\com.rinadown.nmh.firefox.json"

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
  { Force-kill rina_down.exe as a fallback in case Restart Manager fails }
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

{ Remove a URL scheme handler (rinadown:// / ed2k:// / magnet:) registered at runtime by
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
  if not RegQueryStringValue(HKCU, 'Software\Classes\RinaDown.TorrentFile\shell\open\command', '', Command) then
    Exit;
  RegisteredExe := ExtractQuotedExe(Command);
  AppExe := ExpandConstant('{app}\{#MyAppExeName}');
  if (RegisteredExe = '') or (CompareText(RegisteredExe, AppExe) <> 0) then
    Exit;
  if RegQueryStringValue(HKCU, 'Software\Classes\.torrent', '', ProgId)
    and (CompareText(ProgId, 'RinaDown.TorrentFile') = 0) then
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\.torrent');
  RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\RinaDown.TorrentFile');
end;

{ Remove the autostart Run value written at runtime by the launch_at_startup
  plugin (lib/main.dart — value name "RinaDown", data `"<exe>" --silentStart`).
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

{ ── 旧品牌（FluxDown）安装目录清理 ─────────────────────────────────────────
  2026-09 项目由 FluxDown 更名为 RinaDown。更名前装出来的程序文件夹
  （…\Programs\FluxDown / C:\Program Files\FluxDown）既不会被新版复用
  （UsePreviousAppDir=no），也不在任何卸载记录里（旧机实测无对应 Uninstall
  键），放着就是一只名叫 FluxDown 的死目录——用户看到的「安装完还是
  FluxDown」正是它。安装成功后删除。

  只在目录确实是我们自己的安装时才动（含 unins000.exe，或含 flux_down.exe，
  或 rina_down.exe + hub.dll），避免误删用户同名的自建目录；用户若恰好把新版
  装在同一路径（AppDir），跳过。 }

function LooksLikeOurInstall(const Dir: String): Boolean;
var
  Prefix: String;
begin
  Prefix := AddBackslash(Dir);
  Result := FileExists(Prefix + 'unins000.exe')
    or FileExists(Prefix + 'flux_down.exe')
    or (FileExists(Prefix + 'rina_down.exe') and FileExists(Prefix + 'hub.dll'));
end;

procedure TryRemoveLegacyBrandDir(const Dir, AppDir: String);
begin
  if CompareText(Dir, AppDir) = 0 then
  begin
    Log('legacy dir is the target dir, keeping it: ' + Dir);
  end
  else if not DirExists(Dir) then
  begin
    { 绝大多数机器上没有旧目录，这里是常态分支。 }
  end
  else if not LooksLikeOurInstall(Dir) then
  begin
    Log('legacy dir left alone (not our install): ' + Dir);
  end
  else if DelTree(Dir, True, True, True) then
  begin
    Log('removed legacy install dir: ' + Dir);
  end
  else
  begin
    Log('legacy install dir not fully removed (files locked?): ' + Dir);
  end;
end;

procedure RemoveLegacyBrandInstallDir;
var
  AppDir: String;
begin
  AppDir := ExpandConstant('{app}');
  { 非管理员安装（PrivilegesRequired=lowest）落在 %LOCALAPPDATA%\Programs，
    管理员安装落在 %ProgramFiles%；两种都查，不存在的那条是空操作。 }
  TryRemoveLegacyBrandDir(ExpandConstant('{localappdata}\Programs\FluxDown'), AppDir);
  TryRemoveLegacyBrandDir(ExpandConstant('{commonpf}\FluxDown'), AppDir);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  { 等安装文件全部就位后再清旧目录：中途失败时至少还有一边是完整可用的。 }
  if CurStep = ssPostInstall then
    RemoveLegacyBrandInstallDir;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    { Chrome/Edge/Firefox Native Messaging Host registrations written at
      runtime by native/hub/src/nmh_registry.rs. Never declared in the
      Registry section (the app writes them directly via winreg on every startup),
      so the standard uninstall never removes them. `com.rinadown.nmh` is
      RinaDown-specific, safe to remove unconditionally. }
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Google\Chrome\NativeMessagingHosts\com.rinadown.nmh');
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Microsoft\Edge\NativeMessagingHosts\com.rinadown.nmh');
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Mozilla\NativeMessagingHosts\com.rinadown.nmh');

    { rinadown:// / ed2k:// / magnet: URL protocol handlers — same gap as above. }
    RemoveProtocolHandler('rinadown');
    RemoveProtocolHandler('ed2k');
    RemoveProtocolHandler('magnet');

    { .torrent association + autostart Run value — runtime-written variants
      of the [Registry] task entries, invisible to the uninstall log. }
    RemoveTorrentAssociation;
    RemoveAutostartRunValue;
  end;
end;
