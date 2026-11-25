#ifndef AppVersion
  #error AppVersion must be supplied on the ISCC command line
#endif
#ifndef AppArch
  #error AppArch must be supplied on the ISCC command line
#endif
#ifndef BundleDir
  #error BundleDir must be supplied on the ISCC command line
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

#if AppArch == "x86_64"
  #define AllowedArchitectures "x64os"
  #define InstallIn64BitMode "x64os"
#elif AppArch == "arm64"
  #define AllowedArchitectures "arm64"
  #define InstallIn64BitMode "arm64"
#else
  #error Unsupported AppArch
#endif

[Setup]
AppId={{4E0DB59B-A9DB-46BD-97B8-DAA2E8336BF9}
AppName=AirEcho
AppVersion={#AppVersion}
AppPublisher=AirEcho
DefaultDirName={localappdata}\Programs\AirEcho
DefaultGroupName=AirEcho
PrivilegesRequired=lowest
ArchitecturesAllowed={#AllowedArchitectures}
ArchitecturesInstallIn64BitMode={#InstallIn64BitMode}
UninstallDisplayIcon={app}\AirEcho.exe
OutputDir={#OutputDir}
OutputBaseFilename=AirEcho_Desktop-{#AppVersion}-{#AppArch}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Files]
Source: "{#BundleDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\AirEcho"; Filename: "{app}\AirEcho.exe"; AppUserModelID: "in.tl2a.airEcho"
Name: "{autodesktop}\AirEcho"; Filename: "{app}\AirEcho.exe"; AppUserModelID: "in.tl2a.airEcho"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\AllFilesystemObjects\shell\AirEcho.Send"; ValueType: string; ValueName: ""; ValueData: "Send with AirEcho"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\AllFilesystemObjects\shell\AirEcho.Send"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\AirEcho.exe,0"
Root: HKCU; Subkey: "Software\Classes\AllFilesystemObjects\shell\AirEcho.Send"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\AllFilesystemObjects\shell\AirEcho.Send\command"; ValueType: string; ValueName: ""; ValueData: """{app}\AirEcho.exe"" ""%*"""; Flags: uninsdeletekey

[Run]
Filename: "{app}\AirEcho.exe"; Description: "Launch AirEcho"; Flags: postinstall nowait skipifsilent
