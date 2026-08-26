# ShaPrint Multi-Platform Design Spec

## Overview

ShaPrint saat ini adalah aplikasi Windows-only (.NET 8 + WPF) untuk sharing printer via LAN. Spec ini mendesain arsitektur multi-platform yang mendukung:

- **Desktop**: Windows, macOS, Linux (Server + Client + Monitor)
- **Android**: Client print-only (kirim file ke server)
- **Web** (future): Upload-to-print

Spec ini sudah direvisi setelah technical review. Perubahan kunci vs. revisi sebelumnya: (1) struktur project dipecah per-TFM agar setiap project punya satu target framework yang bersih, (2) backend Unix memakai strategi CLI-first CUPS/SANE (tanpa P/Invoke ke shared library CUPS/SANE di v1), (3) jalur kirim job Unix ditutup dengan verb CLI `shaprint send`, dan (4) semua binding UI mengikuti model `ShaPrint.Core` yang aktual.

## Scope & Constraints

### In Scope
1. Server mode di semua desktop platform
2. Client mode di semua desktop platform (Windows Spooler, macOS/Linux CUPS)
3. Monitor mode di semua desktop platform
4. Android: print-only (pilih server -> upload file -> print)
5. Shared UI codebase via Avalonia UI

### Out of Scope (for now)
- Web upload-to-print (akan di-spec terpisah)
- Mobile scanner support
- iOS support
- ImageCapture framework di macOS (v1 memakai SANE/`scanimage`; lihat Future Work)
- `ISystemIntegration` sebagai interface formal (lihat Future Work)

### Key Constraints
- `ShaPrint.Core` harus tetap platform-agnostic dan reusable 100%
- Networking protocol (TCP/UDP) tidak berubah
- Security model (AES-256-GCM, HMAC-SHA256, PBKDF2) tetap sama
- Backward compatible dengan ShaPrint Windows yang sudah ada
- Driver provisioning pipeline (Canon/HP/Brother) dan `DriverSafetyGuard` TIDAK dibuang; dipindahkan dan tetap di jalur install virtual printer Windows

---

## Arsitektur

### Layer Diagram

```
+---------------------------------------------------------------+
|                        ShaPrint.UI                            |
|          (Avalonia - Shared Desktop + Android)                 |
|   Views (XAML) | ViewModels (shared) | Services (shared)      |
|   Program.cs (single entry point, runtime DI switch)          |
+---------------------------------------------------------------+
                          |
                          |  calls platform interfaces (DI)
                          v
+---------------------------------------------------------------+
|               ShaPrint.Platform.Abstractions                   |
|   IPrinterManager | IScannerService | IStartupManager         |
|   INotificationService | IFirewallManager                     |
|   IVirtualPrinterManager | IPrintRelayClient                  |
+---------------------------------------------------------------+
          |                                   |
          v                                   v
+----------------------------+   +----------------------------+
|  ShaPrint.Platform.Windows |   |  ShaPrint.Platform.Unix    |
|  (net8.0-windows10.0.17763)|   |  (net8.0, macOS + Linux)   |
+----------------------------+   +----------------------------+
          |                                   |
          v                                   v
+---------------------------------------------------------------+
|                        ShaPrint.Core                          |
|            (Platform-Agnostic - unchanged)                     |
|   Networking | Crypto | Protocol | Models/DTOs                |
+---------------------------------------------------------------+
```

Catatan: `ISystemIntegration` yang ada di revisi lama TIDAK dipertahankan sebagai interface pada layer abstraksi. Ia dipindah ke "Future Work" agar daftar interface di arsitektur sama persis dengan daftar interface di implementation plan.

### Project Structure

```
ShaPrint.sln
+-- ShaPrint.Core/                        # Existing, no changes
|   +-- Network/                          # PrinterInfo, ScannerInfo, MonitorModels,
|   |                                     #   PrintJobPayload, DiscoveryResponseMessage
|   +-- CryptoHelper.cs
|   +-- Constants.cs                      # DiscoveryUdpPort=9876, PrintTcpPort=9877,
|   |                                     #   MonitorTcpPort=9878, SharedSecret, SetNetworkChannel()
|   `-- Validators.cs
|
+-- ShaPrint.Platform.Abstractions/       # NEW - pure interfaces (net8.0)
|   +-- IPrinterManager.cs
|   +-- IVirtualPrinterManager.cs
|   +-- IScannerService.cs
|   +-- IStartupManager.cs
|   +-- INotificationService.cs           # + ToastAction type
|   +-- IFirewallManager.cs
|   `-- IPrintRelayClient.cs
|
+-- ShaPrint.Platform.Windows/            # NEW - net8.0-windows10.0.17763
|   +-- Adapters/                         # interface implementations
|   |   +-- WindowsPrinterManager.cs      # wraps SpoolerApi
|   |   +-- WindowsVirtualPrinterManager.cs # wraps VirtualPrinterManager
|   |   +-- WindowsScannerService.cs      # wraps ScannerService (WIA)
|   |   +-- WindowsStartupManager.cs      # wraps Utils/StartupManager
|   |   +-- WindowsNotificationService.cs # wraps NotificationService
|   |   +-- WindowsFirewallManager.cs     # wraps FirewallManager
|   |   `-- WindowsPrintRelayClient.cs    # implements IPrintRelayClient (PipeListener path)
|   `-- Services/                         # git mv'd from ShaPrint.WpfApp/Services
|       +-- SpoolerApi.cs, ScannerService.cs, FirewallManager.cs
|       +-- VirtualPrinterManager.cs, PipeListener.cs
|       +-- DriverInstaller.cs, DriverPackageManager.cs, DriverNameResolver.cs,
|       |   DriverPackageVerify.cs, SafeZipExtractor.cs, DriverSafetyGuard.cs,
|       |   RealProcessRunner.cs
|       `-- NotificationService.cs, INotificationService.cs (root), Utils/StartupManager.cs
|
+-- ShaPrint.Platform.Unix/               # NEW - net8.0 (macOS + Linux, runtime guard)
|   +-- UnixPrinterManager.cs             # CLI: lpstat -p / lp -d / lpadmin / lpinfo
|   +-- UnixVirtualPrinterManager.cs      # CUPS backend + PPD, privilege escalation
|   +-- UnixScannerService.cs             # SANE via scanimage CLI
|   +-- UnixStartupManager.cs             # LaunchAgent (macOS) / systemd user (Linux)
|   +-- UnixNotificationService.cs        # osascript / notify-send
|   +-- UnixFirewallManager.cs            # pfctl / ufw / firewall-cmd / iptables (best-effort)
|   `-- UnixPrintRelayClient.cs           # CLI sender path (shared logic)
|
+-- ShaPrint.Android/                     # NEW - separate project (net8.0-android)
|   +-- MainActivity.cs                   # : AvaloniaMainActivity
|   +-- AndroidManifest.xml
|   +-- Services/                         # discovery + multicast lock + print relay
|   `-- Resources/
|
+-- ShaPrint.UI/                          # NEW - Avalonia 11.2.x
|   +-- ShaPrint.UI.csproj                # multi-TFM: net8.0;net8.0-windows
|   +-- Program.cs                        # SINGLE entry point (GUI + CLI `send` branch)
|   +-- app.manifest                      # created together with csproj if referenced
|   +-- App.axaml / App.axaml.cs
|   +-- Services/                         # shared/injectable services (see I below)
|   +-- ViewModels/                       # migrated real code from WpfApp/ViewModels
|   |   +-- MainWindowViewModel.cs
|   |   `-- Pages/ (Server, Client, Monitor, Scan, Settings, Updates, Welcome)
|   `-- Views/                            # Avalonia XAML (compiled bindings, x:DataType)
|
+-- ShaPrint.Updater/                     # Existing, minimal changes
`-- ShaPrint.Tests/                       # Extended with platform contract tests
```

---

## Platform Abstraction Interfaces

Semua interface berikut berada di `ShaPrint.Platform.Abstractions` (namespace `ShaPrint.Platform.Abstractions`) dan hanya bergantung pada tipe `ShaPrint.Core` (atau tipe .NET murni). Tidak ada dependensi ke WPF/Win32 pada layer ini.

### IPrinterManager

```csharp
public interface IPrinterManager
{
    Task<List<PrinterInfo>> GetLocalPrintersAsync();
    Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null);
}
```

- **Windows**: wraps `SpoolerApi` (winspool.drv P/Invoke), existing.
- **macOS/Linux**: CLI-first CUPS: enumerasi `lpstat -p`, print `lp -d <name> -t <title> <file>`.

### IVirtualPrinterManager

Direvisi: TANPA parameter `pipeName`. Nama pipe adalah detail implementasi Windows dan tidak boleh bocor ke abstraksi. Bentuk akhir:

```csharp
public interface IVirtualPrinterManager
{
    Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string driverName);
    Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string virtualPrinterName);
    bool CheckPrinterExists(string printerName);
    List<string> GetInstalledDrivers();
    List<string> GetInstalledVirtualPrinters();
}
```

- `GetInstalledDrivers()`: untuk dialog pilihan driver.
- `GetInstalledVirtualPrinters()`: daftar printer ShaPrint yang terpasang (mis. filter prefix nama).
- **Windows**: implementasi menurunkan nama pipe secara internal (mis. dari `virtualPrinterName`) dan memanggil `VirtualPrinterManager` + pipeline provisioning multi-vendor (Canon/HP/Brother) + `DriverSafetyGuard`.
- **macOS/Linux**: CUPS backend script + PPD + `lpadmin`. Penulisan ke `/usr/lib/cups/backend` dan `/usr/share/cups/model` butuh root; lihat strategi privilege di bagian Unix.

### IScannerService

```csharp
public interface IScannerService
{
    List<ScannerInfo> GetLocalScanners();
    byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, out string actualFormat);
}
```

- **Windows**: WIA 2.0 (existing `ScannerService`).
- **macOS/Linux (v1)**: SANE via CLI `scanimage` (lihat bagian Unix; macOS membutuhkan `brew install sane-backends`).

### IStartupManager

```csharp
public interface IStartupManager
{
    void SetStartup(bool enable);
    bool IsStartupEnabled();
}
```

- **Windows**: Task Scheduler (existing `Utils/StartupManager`).
- **macOS**: LaunchAgent plist (`~/Library/LaunchAgents/`).
- **Linux**: systemd user service (`~/.config/systemd/user/`).

### INotificationService

```csharp
public record ToastAction(string ActivationType, string Arguments);

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

- **Windows**: Windows Toast (existing `NotificationService`, Microsoft.Toolkit.Uwp.Notifications).
- **macOS**: `osascript` / `terminal-notifier`.
- **Linux**: `notify-send` (libnotify).
- **Android**: Android `NotificationManager`.

### IFirewallManager

```csharp
public interface IFirewallManager
{
    Task EnsureFirewallRulesAsync();
}
```

- **Windows**: netsh (existing `FirewallManager`).
- **macOS/Linux**: best-effort `pfctl` / `ufw` / `firewall-cmd` / `iptables`, atau prompt user jika butuh root (lihat bagian Unix).

### IPrintRelayClient

Interface baru untuk jalur kirim job terenkripsi dari client ke server. Dipakai oleh dua implementasi: Windows (`PipeListener` path) dan Unix (CLI sender path).

```csharp
public interface IPrintRelayClient
{
    Task<bool> SendAsync(
        string targetPrinter,
        byte[] data,
        string documentName,
        string? hostOverride = null,
        CancellationToken ct = default);
}
```

Implementasi menggunakan `DiscoveryClient` + `PrintJobPayload` (AES-256-GCM) dari `ShaPrint.Core`, lalu mengirim via TCP port 9877.

---

## Networking Layer

### Current Protocol (Unchanged)

| Port | Protocol | Purpose |
|------|----------|---------|
| 9876 | UDP | Service discovery (broadcast) |
| 9877 | TCP | Print/scan data transfer |
| 9878 | TCP | Monitor status query |

### Client-Server Flow (Desktop Windows + Unix)

```
+---------+            UDP 9876            +---------+
| Client  | ------------------------------>| Server  |
|         | <------------------------------|         |
|         |   Discovery Response (HMAC)    |         |
|         |                                |         |
|         |            TCP 9877            |         |
|         | ------------------------------>|         |
|         |   Print Job Payload (AES-GCM)  |         |
+---------+                                +---------+
```

### Print Relay Flow (IPrintRelayClient)

Jalur ini dipakai ketika print job harus diteruskan dari virtual printer lokal ke server ShaPrint yang mengekspos printer target:

```
CUPS backend script (Unix)  OR  PipeListener (Windows)
             |
             v
   write job to temp file / capture spool data
             |
             v
   IPrintRelayClient.SendAsync(targetPrinter, data, documentName, hostOverride)
             |
             v
   DiscoveryClient resolves server + host (unless hostOverride)
             |
             v
   PrintJobPayload.WriteAsync() -> AES-256-GCM -> TCP 9877
             |
             v
   (Unix) temp file deleted; (Windows) pipe closed
```

### CLI Sender (Unix)

Backend CUPS mengeksekusi verb CLI:

```
shaprint send --printer <nama> --file <path> [--host <ip>]
```

Verb `send` diproses di `Program.cs` `ShaPrint.UI` sebagai branch CLI SEBELUM memulai GUI (bukan entry point terpisah). CLI membangun `IPrintRelayClient` + `DiscoveryClient` dan mengirim `PrintJobPayload` terenkripsi.

### Android Print Flow

Android tidak bisa membuat virtual printer; flow-nya:

```
+---------+            TCP 9877            +---------+
| Android | ------------------------------>| Server  |
|  App    |   Print Job Payload (AES-GCM)  | Desktop |
+---------+                                +---------+
```

Android app:
1. Discovery server via UDP broadcast (memakai `WifiManager.MulticastLock`)
2. User memilih target printer
3. User memilih file via Storage Access Framework (`ACTION_OPEN_DOCUMENT`)
4. Kirim file ke server via TCP 9877 (`IPrintRelayClient`)

---

## UI Architecture (Avalonia)

### Why Avalonia

| Criteria | Avalonia | MAUI | Electron |
|----------|----------|------|----------|
| Cross-platform desktop | Ya (Win/Mac/Linux) | Limited Linux | Ya |
| Android support | Ya | Ya | Tidak |
| XAML familiarity | Ya (WPF-like) | Ya | Tidak |
| Bundle size | Kecil | Kecil | Besar (Chromium) |
| Native look | Ya | Ya | Tidak |
| .NET ecosystem | Ya | Ya | Tidak |

### Single Entry Point (Program.cs)

Konsep file entry point terpisah per-OS DIBUANG. Pola multi entry point itu menyebabkan CS0017 (multiple entry points). Sebagai gantinya:

```csharp
// ShaPrint.UI/Program.cs - SATU entry point
public static class Program
{
    [STAThread]
    public static void Main(string[] args)
    {
        // CLI branch: `shaprint send ...` sebelum GUI
        if (CliDispatcher.TryHandle(args)) return;

        BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
    }

    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .LogToTrace();

    public static void ConfigureServices(IServiceCollection services)
    {
        if (OperatingSystem.IsWindows())
        {
            services.AddPlatformWindows();
        }
        else if (OperatingSystem.IsMacOS() || OperatingSystem.IsLinux())
        {
            services.AddPlatformUnix();
        }
        // Android: services configured via static callback from MainActivity (see Android section)
    }
}
```

- csproj multi-TFM: `net8.0;net8.0-windows`. TFM `net8.0-windows` dipakai agar project dapat mereferensikan `ShaPrint.Platform.Windows`; `net8.0` + `Avalonia.Desktop` untuk Unix.
- Jika csproj memakai `<ApplicationManifest>`, file `app.manifest` HARUS dibuat di task yang sama (tidak boleh menunjuk file yang tidak ada).

### DI Registration (Runtime)

Registrasi DI platform dipilih RUNTIME di `Program.cs` menggunakan `OperatingSystem.IsWindows()/IsMacOS()/IsLinux()`. `App.Host` dibangun TEPAT SATU kali (di `App.OnFrameworkInitializationCompleted`, atau sekali di `Program.cs` lalu di-inject). Tidak boleh ada dua `Host`.

### ViewModel Migration Strategy

- `ObservableObject` / `CommunityToolkit.Mvvm` tetap sama.
- Hapus dependensi `System.Windows` dan `WPF-UI`; ganti dengan abstraksi `ShaPrint.Platform.Abstractions`.
- `Hardcodet.NotifyIcon` -> Avalonia built-in `TrayIcon`.
- Migrasi KODE NYATA (bukan stub): `Server`, `Client`, `Monitor`, `Scan`, `Settings`, `Updates`, `Welcome`, `MainWindow`. Fitur yang implementasinya sudah ada (`ScanLan`, `CheckForUpdates`, `Start/StopServer`) TIDAK boleh diganti `Task.Delay` placeholder. Placeholder hanya untuk fitur yang memang ditunda, dan wajib ditandai `// deferred: ...`.

### Printer/Scanner Selection (Avalonia)

Model `PrinterInfo` / `ScannerInfo` di `ShaPrint.Core` TIDAK punya properti seleksi, dan TIDAK boleh ditambahi properti UI. Seleksi dilakukan via:

- `ListBox SelectionMode="Toggle"` (atau `"Multiple,Toggle"`) dengan `SelectedItems` di-bind ke collection di ViewModel, ATAU
- wrapper item ViewModel per baris.

Semua binding harus valid terhadap `x:DataType` (compiled bindings).

---

## Platform-Specific Implementation Details

### Windows (Existing - Refactor Only)

Fase ekstraksi TIDAK mengubah perilaku Windows. File existing di-`git mv` (bukan salin ulang) dari `ShaPrint.WpfApp/Services` ke `ShaPrint.Platform.Windows`:

- `SpoolerApi.cs` -> adapter `WindowsPrinterManager`
- `VirtualPrinterManager.cs` -> adapter `WindowsVirtualPrinterManager`
- `ScannerService.cs` -> adapter `WindowsScannerService`
- `FirewallManager.cs` -> adapter `WindowsFirewallManager`
- `Utils/StartupManager.cs` -> adapter `WindowsStartupManager`
- `NotificationService.cs` + `INotificationService.cs` -> adapter `WindowsNotificationService`
- `PipeListener.cs` -> `WindowsPrintRelayClient` (IPrintRelayClient)

Pipeline driver provisioning multi-vendor dan hardening IKUT PINDAH dan TETAP di jalur install virtual printer:
`DriverInstaller`, `DriverPackageManager`, `DriverNameResolver`, `DriverPackageVerify`, `SafeZipExtractor`, `RealProcessRunner`, dan `DriverSafetyGuard` (cegah BSOD/kernel corruption/spooler deadlock).

Setelah fase ini `ShaPrint.WpfApp` mereferensikan `ShaPrint.Platform.Windows` dan seluruh test `ShaPrint.Tests` tetap hijau.

### Unix (macOS + Linux) - Single Project, CLI-first

`ShaPrint.Platform.Unix` (net8.0) menggabungkan backend macOS dan Linux dalam SATU project dengan guard runtime `OperatingSystem.IsMacOS()` / `OperatingSystem.IsLinux()`. Strategi CLI-first: TANPA P/Invoke ke shared library CUPS di v1. Ini sekaligus menghapus bug marshaling `cups_dest_t` yang salah menghitung stride (32 vs 40 byte).

| Operasi | CLI |
|---------|-----|
| Enumerasi printer | `lpstat -p` |
| Print | `lp -d <name> -t <title> <file>` |
| Administrasi (install/remove virtual printer) | `lpadmin` |
| Enumerasi driver/PPD | `lpinfo -m` |

**Virtual Printer (CUPS Backend)**:

Backend script (`/usr/lib/cups/backend/shaprint`) TIDAK boleh membuang data. Alur resmi:

```bash
#!/bin/bash
# /usr/lib/cups/backend/shaprint
# $1 = job-id, $2 = user, $3 = title, $4 = copies, $5 = options, [$6 = printer name]
PRINTER="$6"
TMP=$(mktemp /tmp/shaprint_job_XXXXXX)
cat > "$TMP"
# Kirim via CLI sender (bukan shell redirect `>` ke ProcessStartInfo.Arguments)
shaprint send --printer "$PRINTER" --file "$TMP"
rc=$?
rm -f "$TMP"
exit $rc
```

- Data job dibaca dari stdin ke temp file, lalu dikirim oleh CLI sender `shaprint send`.
- CLI sender memakai `IPrintRelayClient` + `DiscoveryClient` + `PrintJobPayload` (AES-256-GCM) dari `ShaPrint.Core`.
- Temp file dihapus setelah kirim.

**Privilege (root)**:

Menulis ke `/usr/lib/cups/backend` dan `/usr/share/cups/model` butuh root. Strategi realistis:
1. Coba tulis; deteksi kegagalan (UnauthorizedAccessException / IOException).
2. Saat gagal, aplikasi menampilkan/mencetak perintah installer yang harus dijalankan user via `sudo`/`pkexec`, ATAU
3. Saat dipaketkan, script `postinst` (Debian/rpm) menulis file tersebut saat install.

DILARANG menganggap `File.WriteAllText` biasa akan berhasil ke path sistem.

**Scanner (v1 = SANE via scanimage)**:

- macOS: prasyarat `brew install sane-backends`.
- Linux: `scanimage` (package `sane-utils`).
- Output `scanimage` dibaca via `RedirectStandardOutput` lalu ditulis ke file oleh kode C#. Karakter `>` di `ProcessStartInfo.Arguments` dengan `UseShellExecute=false` TIDAK berfungsi sebagai shell redirect dan DILARANG dipakai lagi.

**Startup**:
- macOS: LaunchAgent plist di `~/Library/LaunchAgents/`.
- Linux: systemd user service di `~/.config/systemd/user/`.

**Notification**:
- macOS: `osascript` / `terminal-notifier`.
- Linux: `notify-send`.

**Firewall** (best-effort, karena butuh root):
- macOS: `pfctl` atau prompt user.
- Linux: `ufw` / `firewall-cmd` / `iptables` atau prompt user.

### Android (Separate Project)

`ShaPrint.Android` adalah project TERPISAH (net8.0-android), bukan folder dalam `ShaPrint.UI`.

- `MainActivity : AvaloniaMainActivity`.
- Host DI dibangun SEKALI. `MainActivity` men-set konfigurasi layanan Android via static callback/delegate SEBELUM Avalonia memanggil `OnFrameworkInitializationCompleted` (tidak ada race dua `Host`).
- UDP discovery memakai `WifiManager.MulticastLock` (acquire saat scan, release setelah selesai).
- Permission HANYA: `INTERNET`, `ACCESS_NETWORK_STATE`, `ACCESS_WIFI_STATE`, `CHANGE_WIFI_MULTICAST_STATE`, `POST_NOTIFICATIONS`.
- Permission storage eksternal (baca/tulis) DIHAPUS; file picker memakai Storage Access Framework (`ACTION_OPEN_DOCUMENT`).
- `minSdkVersion=21`, `targetSdkVersion=34`, `versionName=2.0.0`.

---

## Migration Strategy (Phased)

### Fase 1: Ekstraksi Platform (tanpa ubah UI)

1. Buat `ShaPrint.Platform.Abstractions` (semua interface + `IPrintRelayClient`).
2. Buat `ShaPrint.Platform.Windows`; `git mv` service existing + adapter.
3. Pertahankan `DriverSafetyGuard` + pipeline provisioning multi-vendor di jalur install.
4. `ShaPrint.WpfApp` mereferensikan `ShaPrint.Platform.Windows`.
5. **Gate**: `ShaPrint.WpfApp` tetap build; `dotnet test ShaPrint.Tests` hijau.

### Fase 2: ShaPrint.UI (Avalonia) + Platform.Unix

1. Skeleton `ShaPrint.UI`: satu `Program.cs`, DI switch runtime, `app.manifest` valid.
2. Migrasi shared services (Discovery/Update/Monitor/PipeListener wiring).
3. Migrasi ViewModels nyata.
4. Views Avalonia dengan binding benar (`SelectionMode`, `x:DataType`).
5. `ShaPrint.Platform.Unix` backends (printer + PPD + backend script + eskalasi privilege, scanner `scanimage`, notification, startup, firewall).
6. CLI sender `send` + wiring backend script.

### Fase 3: ShaPrint.Android

1. Project terpisah `ShaPrint.Android`.
2. `MainActivity`, multicast lock, SAF file picker, permissions.
3. Test Android -> Windows server flow.

### Fase 4: CI + Packaging + Docs

1. CI matrix (windows-latest, ubuntu-latest, macos-latest) + contract tests.
2. Packaging per platform.
3. Docs (`multi-platform-guide`) + README.
4. Release v2.0.

---

## Testing Strategy

### Unit Tests
- `ShaPrint.Core`: Existing tests (no changes).
- `ShaPrint.Platform.Abstractions`: contract tests (interface shape) - lihat Contract Tests.
- `ShaPrint.UI`: ViewModel tests dengan mocked platform services.

### Contract Tests (ShaPrint.Tests)

Test yang TIDAK bergantung OS: setiap implementasi platform harus memenuhi kontrak interface. Contract tests didefinisikan terhadap interface dan di-run terhadap implementasi yang tersedia pada runner. Ini memungkinkan verifikasi `IPrintRelayClient` (serialisasi `PrintJobPayload`, validasi ukuran), `IVirtualPrinterManager` (tidak ada `pipeName` di signature), dan shape `ServerStatusPayload`.

### Integration Tests
- Server <-> Client communication (cross-platform)
- Discovery protocol (UDP broadcast)
- Print job flow (end-to-end)
- Backend CUPS script -> CLI sender -> server (Unix, manual/CI)

### CI Matrix

| Runner | Yang diverifikasi |
|--------|-------------------|
| `windows-latest` | `ShaPrint.WpfApp` build, `ShaPrint.Platform.Windows`, `ShaPrint.Tests`, `ShaPrint.UI` (net8.0-windows) |
| `ubuntu-latest` | `ShaPrint.Platform.Unix`, `ShaPrint.UI` (net8.0), `ShaPrint.Tests` |
| `macos-latest` | `ShaPrint.Platform.Unix`, `ShaPrint.UI` (net8.0), `ShaPrint.Tests` |

Android diverifikasi via `dotnet build -f net8.0-android` pada `ubuntu-latest` (atau `windows-latest`) + emulator/physical device manual.

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| CUPS backend complexity on macOS/Linux | High | CLI-first `lp`/`lpadmin`/`lpinfo`, iterate |
| **Privilege Unix (tulis /usr/lib/cups/backend, /usr/share/cups/model)** | **High** | **Deteksi kegagalan tulis; tampilkan perintah sudo/pkexec; postinst saat packaging. Tidak menganggap File.WriteAllText berhasil** |
| Avalonia Android maturity | Medium | Avalonia 11.x has stable Android support |
| WIA -> SANE rewrite | Medium | Abstract early; `scanimage` CLI di v1; ImageCapture ke Future Work |
| CLI sender/backend script integration | Medium | Verb `send` eksplisit + integration test backend->CLI->server |
| Bundle size on Android | Low | Avalonia produces small APKs |
| Backward compatibility | Medium | Keep protocol unchanged, test with old clients |

---

## Decisions

1. **macOS Scanner v1**: SANE via `scanimage` (prasyarat `brew install sane-backends`). ImageCapture framework dipindah ke Future Work.
2. **Linux Scanner**: SANE via `scanimage` CLI (package `sane-utils`), bukan P/Invoke library SANE.
3. **CUPS (macOS/Linux)**: CLI-first (`lpstat`/`lp`/`lpadmin`/`lpinfo`), BUKAN P/Invoke library CUPS di v1.
4. **Project layout**: dipisah per-TFM (`Abstractions`, `Platform.Windows`, `Platform.Unix`, `Android` terpisah, `UI` multi-TFM).
5. **Android**: project TERPISAH (`ShaPrint.Android`), bukan folder dalam `ShaPrint.UI`.
6. **Client print path Unix**: backend script -> temp file -> CLI sender `shaprint send` -> `IPrintRelayClient` -> `PrintJobPayload` terenkripsi. Tidak ada pola `cat > /tmp ... rm` yang membuang data.
7. **Android file types**: PDF dan image only.
8. **Auto-update**: Tetap GitHub-based untuk sekarang.

---

## Future Work

- **ISystemIntegration**: abstraksi formal untuk integrasi level sistem (di luar printer/scanner/notification/firewall/startup/relay) ditunda sampai kebutuhan nyata muncul.
- **macOS ImageCapture framework**: pengganti SANE untuk kualitas/integrasi lebih baik di macOS (setelah v1 SANE berjalan).
- **P/Invoke shared library CUPS / SANE**: dapat dipertimbangkan setelah CLI-first stabil, dengan hati-hati pada marshaling `cups_dest_t` (stride 32 vs 40 byte).
- **Web upload-to-print**: spec terpisah.

---

## Appendix: Dependency Map

### Current Dependencies (WPF)
```
ShaPrint.WpfApp
+-- CommunityToolkit.Mvvm 8.4.2 (ViewModels)
+-- FontAwesome.Sharp 6.6.0 (Icons)
+-- Hardcodet.NotifyIcon.Wpf 2.0.1 (System tray)
+-- Microsoft.Extensions.Hosting 10.0.8 (DI)
+-- Microsoft.Toolkit.Uwp.Notifications 7.1.3 (Toast)
+-- System.IO.Pipes.AccessControl 5.0.0 (Named pipes)
`-- WPF-UI 4.3.0 (Fluent theme)
```

### New Dependencies (Avalonia)
```
ShaPrint.UI (multi-TFM: net8.0;net8.0-windows)
+-- Avalonia 11.2.x
+-- Avalonia.Desktop (Windows/macOS/Linux)
+-- Avalonia.Themes.Fluent
+-- Avalonia.Fonts.Inter
+-- CommunityToolkit.Mvvm 8.4.2 (ViewModels - same)
+-- FluentAvalonia (Fluent theme - replaces WPF-UI)
`-- Microsoft.Extensions.Hosting 10.0.8 (DI - same)
```

### Platform Dependencies
```
ShaPrint.Platform.Abstractions  -> ShaPrint.Core (types only)
ShaPrint.Platform.Windows       -> ShaPrint.Core + Microsoft.Toolkit.Uwp.Notifications
                                   + System.IO.Pipes.AccessControl (net8.0-windows10.0.17763)
ShaPrint.Platform.Unix          -> ShaPrint.Core (CLI-first, no P/Invoke in v1)
ShaPrint.Android                -> ShaPrint.Core + Avalonia.Android (net8.0-android)
```
