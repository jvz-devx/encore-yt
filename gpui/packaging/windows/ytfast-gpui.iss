; Inno Setup script for Music (ytfast-gpui). Built by .github/workflows/release.yml:
;   iscc /DAppVersion=<version> /DSourceDir=<dir with the files> /DOutputDir=<dir> ytfast-gpui.iss
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
AppName=Music (ytfast)
AppVersion={#AppVersion}
AppPublisher=jvz-devx
AppPublisherURL=https://github.com/jvz-devx/ytfast-gpui
DefaultDirName={localappdata}\Programs\ytfast-gpui
DefaultGroupName=Music (ytfast)
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=ytfast-gpui-{#AppVersion}-windows-x86_64-setup
SetupIconFile={#SourceDir}\ytfast-gpui.ico
UninstallDisplayIcon={app}\ytfast-gpui.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\ytfast-gpui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\ytfast-gpui.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\THIRD-PARTY.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\bin\*"; DestDir: "{app}\bin"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\Music (ytfast)"; Filename: "{app}\ytfast-gpui.exe"; IconFilename: "{app}\ytfast-gpui.ico"
Name: "{autodesktop}\Music (ytfast)"; Filename: "{app}\ytfast-gpui.exe"; IconFilename: "{app}\ytfast-gpui.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\ytfast-gpui.exe"; Description: "{cm:LaunchProgram,Music}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{cmd}"; Parameters: "/C taskkill /IM ytfast-gpui.exe /F"; Flags: runhidden; RunOnceId: "StopMusic"
