#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#ifndef BinaryPath
  #define BinaryPath "..\..\target\release\texreq.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif
#ifndef OutputBaseFilename
  #define OutputBaseFilename "texreq-v0.1.0-windows-x86_64-setup"
#endif

[Setup]
AppId={{9B2D9E7A-3C41-4A9D-8B74-5F3C1E8D42A0}
AppName=TexReq
AppVersion={#MyAppVersion}
AppVerName=TexReq {#MyAppVersion}
AppPublisher=Textic
AppPublisherURL=https://github.com/Textic/TexReq
AppSupportURL=https://github.com/Textic/TexReq/releases
DefaultDirName={autopf}\TexReq
DefaultGroupName=TexReq
AllowNoIcons=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
SetupIconFile=..\..\assets\texreq.ico
UninstallDisplayIcon={app}\texreq.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
VersionInfoVersion={#MyAppVersion}.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BinaryPath}"; DestDir: "{app}"; DestName: "texreq.exe"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\PLAN.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\texreq.ico"; DestDir: "{app}\assets"; Flags: ignoreversion

[Icons]
Name: "{group}\TexReq"; Filename: "{app}\texreq.exe"; IconFilename: "{app}\texreq.exe"
Name: "{group}\{cm:UninstallProgram,TexReq}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\TexReq"; Filename: "{app}\texreq.exe"; IconFilename: "{app}\texreq.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\texreq.exe"; Description: "{cm:LaunchProgram,TexReq}"; Flags: nowait postinstall skipifsilent
