# ShaPrint Multi-Platform Design Spec

## Overview

ShaPrint saat ini adalah aplikasi Windows-only (.NET 8 + WPF) untuk sharing printer via LAN. Spec ini mendesain arsitektur multi-platform yang mendukung:

- **Desktop**: Windows, macOS, Linux (Server + Client + Monitor)
- **Android**: Client print-only (kirim file ke server)
- **Web** (future): Upload-to-print

## Scope & Constraints

### In Scope
1. Server mode di semua desktop platform
2. Client mode di semua desktop platform (Windows Spooler, macOS/Linux CUPS)
3. Monitor mode di semua desktop platform
4. Android: print-only (pilih server → upload file → print)
5. Shared UI codebase via Avalonia UI

### Out of Scope (for now)
- Web upload-to-print (akan di-spec terpisah)
- Mobile scanner support
- iOS support

### Key Constraints
- `ShaPrint.Core` harus tetap platform-agnostic dan reusable 100%
- Networking protocol (TCP/UDP) tidak berubah
- Security model (AES-256-GCM, HMAC-SHA256, PBKDF2) tetap sama
- Backward compatible dengan ShaPrint Windows yang sudah ada

---

## Arsitektur

### Layer Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                        ShaPrint.UI                              │
│              (Avalonia - Shared Desktop + Android)               │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │  Views   │ │ViewModels│ │ Services │ │ Platform Adapters │   │
│  │ (XAML)   │ │ (shared) │ │ (shared) │ │  (per-platform)  │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     ShaPrint.Platform                           │
│                  (Platform Abstraction Layer)                    │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐    │
│  │IPrinterManager│ │IScannerService│ │IStartupManager     │    │
│  │INotification  │ │IFirewallManager│ │ISystemIntegration  │    │
│  └──────────────┘ └──────────────┘ └──────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                       ShaPrint.Core                             │
│                    (Platform-Agnostic)                           │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │Networking│ │  Crypto  │ │ Protocol │ │    Models/DTOs   │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### Project Structure

```
ShaPrint.sln
├── ShaPrint.Core/                    # Existing, no changes
│   ├── Abstractions/
│   ├── Network/
│   ├── CryptoHelper.cs
│   ├── Constants.cs
│   └── Validators.cs
│
├── ShaPrint.Platform/                # NEW - Platform abstraction
│   ├── Abstractions/
│   │   ├── IPrinterManager.cs        # Enumerate, print raw data
│   │   ├── IScannerService.cs        # List scanners, perform scan
│   │   ├── IStartupManager.cs        # Auto-start on boot
│   │   ├── INotificationService.cs   # System notifications
│   │   ├── IFirewallManager.cs       # Port management
│   │   └── IVirtualPrinterManager.cs # Install/remove virtual printer
│   │
│   ├── Windows/
│   │   ├── WindowsPrinterManager.cs  # Wraps SpoolerApi
│   │   ├── WindowsScannerService.cs  # Wraps WIA
│   │   ├── WindowsStartupManager.cs  # Wraps Task Scheduler
│   │   ├── WindowsNotificationService.cs # Toast notifications
│   │   ├── WindowsFirewallManager.cs # netsh commands
│   │   └── WindowsVirtualPrinterManager.cs # PowerShell spooler
│   │
│   ├── macOS/
│   │   ├── MacPrinterManager.cs      # CUPS via libcups
│   │   ├── MacScannerService.cs      # ImageCapture framework
│   │   ├── MacStartupManager.cs      # LaunchAgent plist
│   │   ├── MacNotificationService.cs # NSUserNotification
│   │   └── MacVirtualPrinterManager.cs # CUPS backend
│   │
│   ├── Linux/
│   │   ├── LinuxPrinterManager.cs    # CUPS via libcups
│   │   ├── LinuxScannerService.cs    # SANE
│   │   ├── LinuxStartupManager.cs    # systemd user service
│   │   ├── LinuxNotificationService.cs # libnotify
│   │   └── LinuxVirtualPrinterManager.cs # CUPS backend
│   │
│   └── Android/
│       ├── AndroidPrinterManager.cs  # Android Print Framework
│       └── AndroidNotificationService.cs # Android notifications
│
├── ShaPrint.UI/                      # NEW - Avalonia UI
│   ├── App.axaml
│   ├── App.axaml.cs
│   ├── ViewModels/                   # Migrated from WpfApp
│   │   ├── MainWindowViewModel.cs
│   │   ├── Pages/
│   │   │   ├── ServerViewModel.cs
│   │   │   ├── ClientViewModel.cs
│   │   │   ├── MonitorViewModel.cs
│   │   │   ├── SettingsViewModel.cs
│   │   │   └── UpdatesViewModel.cs
│   │   └── Services/
│   │       ├── DiscoveryClient.cs    # Shared networking
│   │       ├── DiscoveryServer.cs
│   │       ├── MonitorService.cs
│   │       └── UpdateService.cs
│   │
│   ├── Views/                        # Avalonia XAML
│   │   ├── MainWindow.axaml
│   │   └── Pages/
│   │       ├── ServerPage.axaml
│   │       ├── ClientPage.axaml
│   │       ├── MonitorPage.axaml
│   │       ├── SettingsPage.axaml
│   │       └── UpdatesPage.axaml
│   │
│   ├── Platforms/
│   │   ├── Windows/
│   │   │   └── Program.cs
│   │   ├── macOS/
│   │   │   └── Program.cs
│   │   ├── Linux/
│   │   │   └── Program.cs
│   │   └── Android/
│   │       ├── MainActivity.cs
│   │       └── Resources/
│   │
│   └── Services/                     # Shared UI services
│       ├── NavigationService.cs
│       └── DialogService.cs
│
├── ShaPrint.Updater/                 # Existing, minimal changes
│
└── ShaPrint.Tests/                   # Extended with platform tests
```

---

## Platform Abstraction Interfaces

### IPrinterManager

```csharp
public interface IPrinterManager
{
    Task<List<PrinterInfo>> GetLocalPrintersAsync();
    Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null);
}
```

**Windows**: Wraps existing `SpoolerApi` (winspool.drv P/Invoke)
**macOS/Linux**: CUPS API via `libcups` P/Invoke or `Process.Start("lp")`

### IVirtualPrinterManager

```csharp
public interface IVirtualPrinterManager
{
    Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string pipeName, string driverName);
    Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string printerName, string pipeName);
    bool CheckPrinterExists(string printerName);
    List<string> GetInstalledDrivers();
}
```

**Windows**: Existing PowerShell-based implementation
**macOS**: CUPS backend filter + PPD installation
**Linux**: CUPS backend filter + PPD installation

### IScannerService

```csharp
public interface IScannerService
{
    List<ScannerInfo> GetLocalScanners();
    byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, out string actualFormat);
}
```

**Windows**: WIA 2.0 (existing implementation)
**macOS**: ImageCapture framework via P/Invoke
**Linux**: SANE via `libsane` P/Invoke

### IStartupManager

```csharp
public interface IStartupManager
{
    void SetStartup(bool enable);
    bool IsStartupEnabled();
}
```

**Windows**: Task Scheduler (existing)
**macOS**: LaunchAgent plist (`~/Library/LaunchAgents/`)
**Linux**: systemd user service (`~/.config/systemd/user/`)

### INotificationService

```csharp
public interface INotificationService
{
    void ShowToast(string title, string body, ToastAction? action = null);
    void ShowPrintJobCompleted(string documentName, string printerName);
    void ShowPrintJobFailed(string documentName, string printerName, string reason);
    void ShowClientConnected(string clientAddress);
    void ShowClientDisconnected(string clientAddress);
    void ShowScanCompleted(string fileName);
    void ShowScanFailed(string errorMessage);
    void ShowPrinterError(string printerName, string errorDescription);
    void ShowSecurityAlert(string message, string detail);
}
```

**Windows**: Windows Toast (existing)
**macOS**: `osascript` or `terminal-notifier`
**Linux**: `notify-send` via libnotify
**Android**: Android NotificationManager

### IFirewallManager

```csharp
public interface IFirewallManager
{
    Task EnsureFirewallRulesAsync();
}
```

**Windows**: netsh (existing)
**macOS**: `pfctl` or prompt user
**Linux**: `ufw` / `iptables` or prompt user

---

## Networking Layer

### Current Protocol (Unchanged)

| Port | Protocol | Purpose |
|------|----------|---------|
| 9876 | UDP | Service discovery (broadcast) |
| 9877 | TCP | Print/scan data transfer |
| 9878 | TCP | Monitor status query |

### Client-Server Flow

```
┌─────────────┐         UDP 9876          ┌─────────────┐
│   Client    │ ──────────────────────────>│   Server    │
│             │ <──────────────────────────│             │
│             │    Discovery Response      │             │
│             │    (HMAC-SHA256 signed)    │             │
│             │                            │             │
│             │         TCP 9877           │             │
│             │ ──────────────────────────>│             │
│             │    Print Job Payload       │             │
│             │    (AES-256-GCM encrypted) │             │
└─────────────┘                            └─────────────┘
```

### Android Print Flow

Android tidak bisa buat virtual printer, jadi flow-nya:

```
┌─────────────┐         TCP 9877           ┌─────────────┐
│   Android   │ ──────────────────────────>│   Server    │
│   App       │    Print Job Payload       │   (Desktop) │
│             │    (AES-256-GCM encrypted) │             │
└─────────────┘                            └─────────────┘
```

Android app akan:
1. Discovery server via UDP broadcast
2. User pilih target printer
3. User pilih file dari storage
4. Kirim file langsung ke server via TCP

---

## UI Architecture (Avalonia)

### Why Avalonia

| Criteria | Avalonia | MAUI | Electron |
|----------|----------|------|----------|
| Cross-platform desktop | ✅ Win/Mac/Linux | ⚠️ Limited Linux | ✅ |
| Android support | ✅ | ✅ | ❌ |
| XAML familiarity | ✅ (WPF-like) | ✅ | ❌ |
| Bundle size | Small | Small | Large (Chromium) |
| Native look | ✅ | ✅ | ❌ |
| .NET ecosystem | ✅ | ✅ | ❌ |

### View Migration Strategy

WPF XAML → Avalonia XAML differences:
- `Window` → `Window` (same)
- `UserControl` → `UserControl` (same)
- `xmlns:local` → `xmlns:vm` (namespace changes)
- `Binding` → `Binding` (same syntax)
- `DataTemplate` → `DataTemplate` (same)
- ResourceDictionary → `ResourceDictionary` (same, minor syntax)
- `Hardcodet.NotifyIcon` → Avalonia's built-in `TrayIcon`
- `WPF-UI` controls → Avalonia's built-in or `FluentAvalonia` theme

### Shared ViewModels

ViewModels dari `ShaPrint.WpfApp` bisa di-reuse dengan minimal perubahan:
- Remove WPF-specific `ICommand` → use `CommunityToolkit.Mvvm` `ICommand` (already used)
- Remove `System.Windows` dependencies → use `ShaPrint.Platform` abstractions
- `ObservableObject` base class tetap sama

---

## Platform-Specific Implementation Details

### Windows (Existing - Refactor Only)

Move existing code to `ShaPrint.Platform.Windows`:
- `SpoolerApi.cs` → `WindowsPrinterManager.cs`
- `VirtualPrinterManager.cs` → `WindowsVirtualPrinterManager.cs`
- `ScannerService.cs` → `WindowsScannerService.cs`
- `FirewallManager.cs` → `WindowsFirewallManager.cs`
- `StartupManager.cs` → `WindowsStartupManager.cs`
- `NotificationService.cs` → `WindowsNotificationService.cs`

### macOS

**Printer Management (CUPS)**:
```csharp
// P/Invoke libcups
[DllImport("libcups")]
static extern int cupsGetDests(out IntPtr dests);

[DllImport("libcups")]
static extern int cupsPrintFile(string name, string filename, string title, int num_options, IntPtr options);
```

**Virtual Printer (CUPS Backend)**:
- Create custom CUPS backend that pipes data to ShaPrint server
- Install PPD file for the virtual printer
- Backend script reads from stdin, sends to server via TCP

**Scanner (ImageCapture)**:
- Use `ICDeviceBrowser` via P/Invoke or call `scanimage` CLI

**Startup (LaunchAgent)**:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.shaprint.app</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Applications/ShaPrint.app/Contents/MacOS/ShaPrint</string>
        <string>--startup</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
```

### Linux

**Printer Management (CUPS)**:
Same as macOS - CUPS is the standard on Linux.

**Virtual Printer (CUPS Backend)**:
```bash
#!/bin/bash
# /usr/lib/cups/backend/shaprint
# CUPS backend that pipes print data to ShaPrint server
cat > /tmp/shaprint_job_$$.tmp
# Send to server via TCP
dotnet ShaPrint send --printer "$PRINTER" --file /tmp/shaprint_job_$$.tmp
rm /tmp/shaprint_job_$$.tmp
```

**Scanner (SANE)**:
```csharp
[DllImport("libsane")]
static extern int sane_init(out int version_code, IntPtr authorize);

[DllImport("libsane")]
static extern int sane_get_devices(out IntPtr device_list, int local_only);
```

**Startup (systemd)**:
```ini
[Unit]
Description=ShaPrint Service
After=network.target

[Service]
Type=simple
ExecStart=/opt/shaprint/ShaPrint --startup
Restart=on-failure

[Install]
WantedBy=default.target
```

### Android

**Scope**: Print-only (no server, no scanner)

**Architecture**:
```
ShaPrint.Android
├── MainActivity.cs
├── Services/
│   ├── DiscoveryService.cs     # UDP broadcast discovery
│   ├── PrintService.cs         # TCP print job sender
│   └── NetworkChannelService.cs # Key management
├── ViewModels/
│   ├── MainViewModel.cs
│   ├── ServerListViewModel.cs
│   └── PrintJobViewModel.cs
└── Views/
    ├── MainView.axaml
    ├── ServerListView.axaml
    └── PrintJobView.axaml
```

**Print Flow**:
1. App discovers servers on network (UDP 9876)
2. User selects target server + printer
3. User picks file from Android storage
4. App reads file, encrypts with AES-256-GCM
5. Sends via TCP 9877 to server

**File Picker**:
Use Android `Intent.ActionOpenDocument` or `Intent.ActionGetContent`

---

## Migration Strategy

### Phase 1: Foundation (Week 1-2)
1. Create `ShaPrint.Platform` project with interfaces
2. Create `ShaPrint.UI` Avalonia project (skeleton)
3. Move platform-agnostic services from WpfApp to UI project
4. Implement `WindowsPrinterManager`, `WindowsScannerService`, etc.

### Phase 2: Desktop UI (Week 3-4)
1. Migrate ViewModels from WpfApp to Avalonia
2. Create Avalonia XAML views (Windows first)
3. Test Windows desktop fully functional
4. Implement macOS platform backends
5. Implement Linux platform backends

### Phase 3: Android (Week 5-6)
1. Add Android target to `ShaPrint.UI`
2. Implement `AndroidPrinterManager`
3. Create Android-specific views (simplified - print only)
4. Test Android → Windows server flow

### Phase 4: Polish & Release (Week 7-8)
1. Platform-specific testing
2. Installer/packaging per platform
3. Documentation updates
4. Release v2.0

---

## Testing Strategy

### Unit Tests
- `ShaPrint.Core`: Existing tests (no changes)
- `ShaPrint.Platform`: Mock interfaces, test each platform implementation
- `ShaPrint.UI`: ViewModel tests with mocked platform services

### Integration Tests
- Server ↔ Client communication (cross-platform)
- Discovery protocol (UDP broadcast)
- Print job flow (end-to-end)

### Platform Tests
- Windows: Existing test suite
- macOS: Manual testing + CI (GitHub Actions macOS runner)
- Linux: Manual testing + CI (GitHub Actions Ubuntu runner)
- Android: Emulator testing + physical device

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| CUPS backend complexity on macOS/Linux | High | Start with simple `lp` command, iterate |
| Avalonia Android maturity | Medium | Avalonia 11.x has stable Android support |
| WIA → SANE/ImageCapture rewrite | Medium | Abstract early, implement per-platform |
| Bundle size on Android | Low | Avalonia produces small APKs |
| Backward compatibility | Medium | Keep protocol unchanged, test with old clients |

---

## Decisions

1. **macOS Scanner**: ImageCapture framework (kualitas terbaik, lebih integrated)
2. **Linux Scanner**: Bundle `libsane` dengan app
3. **Android file types**: PDF dan image only
4. **Auto-update**: Tetap GitHub-based untuk sekarang

---

## Appendix: Dependency Map

### Current Dependencies (WPF)
```
ShaPrint.WpfApp
├── CommunityToolkit.Mvvm (ViewModels)
├── FontAwesome.Sharp (Icons)
├── Hardcodet.NotifyIcon.Wpf (System tray)
├── Microsoft.Extensions.Hosting (DI)
├── Microsoft.Toolkit.Uwp.Notifications (Toast)
├── System.IO.Pipes.AccessControl (Named pipes)
└── WPF-UI (Fluent theme)
```

### New Dependencies (Avalonia)
```
ShaPrint.UI
├── Avalonia (UI framework)
├── Avalonia.Desktop (Windows/macOS/Linux)
├── Avalonia.Android (Android)
├── CommunityToolkit.Mvvm (ViewModels - same)
├── FluentAvalonia (Fluent theme - replaces WPF-UI)
└── Microsoft.Extensions.Hosting (DI - same)
```

### Platform Dependencies
```
ShaPrint.Platform.Windows
└── System.Printing (WPF only, for print queue queries)

ShaPrint.Platform.macOS
└── (P/Invoke libcups, no NuGet)

ShaPrint.Platform.Linux
└── (P/Invoke libcups, libsane, no NuGet)

ShaPrint.Platform.Android
└── (Android SDK bindings, no NuGet)
```
