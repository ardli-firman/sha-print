#ifndef MyAppVersion
  #define MyAppVersion "1.0.0"
#endif

#ifndef MyAppOutput
  #define MyAppOutput "ShaPrint_Setup_v1.0.0"
#endif

[Setup]
AppName=ShaPrint
AppVersion={#MyAppVersion}
AppPublisher=ShaPrint Open Source
DefaultDirName={autopf}\ShaPrint
DefaultGroupName=ShaPrint
UninstallDisplayIcon={app}\ShaPrint.WpfApp.exe
SetupIconFile=ShaPrint.WpfApp\icon.ico
Compression=lzma2
SolidCompression=yes
OutputDir=Output
OutputBaseFilename={#MyAppOutput}
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional icons:"

[Files]
Source: "ShaPrint.WpfApp\bin\Release\net8.0-windows\win-x64\publish\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "ShaPrint.Updater\bin\Release\net8.0-windows\win-x64\publish\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\ShaPrint"; Filename: "{app}\ShaPrint.WpfApp.exe"
Name: "{group}\Uninstall ShaPrint"; Filename: "{uninstallexe}"
Name: "{commondesktop}\ShaPrint"; Filename: "{app}\ShaPrint.WpfApp.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ShaPrint.WpfApp.exe"; Description: "Launch ShaPrint"; Flags: nowait postinstall shellexec
