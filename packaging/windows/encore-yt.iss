; Inno Setup script for Encore (encore-yt). Built by .github/workflows/release.yml:
;   iscc /DAppVersion=<version> /DSourceDir=<dir with the files> /DOutputDir=<dir> encore-yt.iss
; Installs for the current user (no administrator rights needed).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\windows"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
AppId={{6F1C2B8E-4D1A-4B7E-9A55-2D3F1E0C7A91}
AppName=Encore
AppVersion={#AppVersion}
AppPublisher=jvz-devx
AppPublisherURL=https://github.com/jvz-devx/encore-yt
DefaultDirName={localappdata}\Programs\encore-yt
; The AppId is the one ytfast-gpui had, so this setup upgrades that install;
; its folder and shortcuts are removed below.
UsePreviousAppDir=no
DefaultGroupName=Encore
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=encore-yt-{#AppVersion}-windows-x86_64-setup
SetupIconFile={#SourceDir}\encore-yt.ico
UninstallDisplayIcon={app}\encore-yt.exe
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\encore-yt.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\encore-yt.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\THIRD-PARTY.txt"; DestDir: "{app}"; Flags: ignoreversion
; The M31 sign-in window helper, when the build made it.
Source: "{#SourceDir}\encore-yt-signin.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[InstallDelete]
; ytfast-gpui, as the app was called before the rename: its folder and shortcuts.
Type: filesandordirs; Name: "{localappdata}\Programs\ytfast-gpui"
Type: files; Name: "{autoprograms}\Music (ytfast).lnk"
Type: files; Name: "{autodesktop}\Music (ytfast).lnk"
; mpv, yt-dlp and deno, bundled by versions before 0.2.
Type: filesandordirs; Name: "{app}\bin"

[Icons]
Name: "{autoprograms}\Encore"; Filename: "{app}\encore-yt.exe"; IconFilename: "{app}\encore-yt.ico"
Name: "{autodesktop}\Encore"; Filename: "{app}\encore-yt.exe"; IconFilename: "{app}\encore-yt.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\encore-yt.exe"; Description: "{cm:LaunchProgram,Encore}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{cmd}"; Parameters: "/C taskkill /IM encore-yt.exe /F"; Flags: runhidden; RunOnceId: "StopEncore"
