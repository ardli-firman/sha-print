# ShaPrint Multi-Platform Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate ShaPrint dari Windows-only WPF app ke multi-platform desktop (Win/Mac/Linux) + Android app menggunakan Avalonia UI, tanpa membuang kerja provisioning driver multi-vendor dan `DriverSafetyGuard` yang sudah ada.

**Architecture:** Project dipecah per-TFM: `ShaPrint.Platform.Abstractions` (interface murni), `ShaPrint.Platform.Windows`, `ShaPrint.Platform.Unix` (macOS+Linux), `ShaPrint.Android` (terpisah), dan `ShaPrint.UI` (Avalonia multi-TFM). `ShaPrint.Core` tetap unchanged.

**Tech Stack:** .NET 8, Avalonia UI 11.2.x, CommunityToolkit.Mvvm, FluentAvalonia, CUPS CLI-first (Mac/Linux), SANE `scanimage` (Mac/Linux).

**Spec:** `docs/superpowers/specs/2026-08-24-multi-platform-design.md`

## Global Constraints

- `ShaPrint.Core` tidak boleh dimodifikasi (platform-agnostic)
- Networking protocol (TCP/UDP ports 9876/9877/9878) tidak berubah
- Security model (AES-256-GCM, HMAC-SHA256, PBKDF2) tetap sama
- Backward compatible dengan ShaPrint Windows existing
- Android: PDF dan image only
- Scanner: macOS/Linux v1 = SANE via `scanimage` (bukan ImageCapture / P/Invoke library SANE)
- CUPS: CLI-first (`lpstat`/`lp`/`lpadmin`/`lpinfo`), bukan P/Invoke library CUPS di v1
- Auto-update: GitHub-based
- Setiap commit per task; tiap task punya perintah verifikasi nyata (bukan asumsi)

## Migration Phases (ringkasan)

- **Fase 1** (Task 1-2): Ekstraksi Platform tanpa ubah UI. `ShaPrint.WpfApp` tetap build, `ShaPrint.Tests` hijau.
- **Fase 2** (Task 3-8): `ShaPrint.UI` Avalonia + `ShaPrint.Platform.Unix` + CLI sender `send`.
- **Fase 3** (Task 9): `ShaPrint.Android`.
- **Fase 4** (Task 10-11): CI + packaging + docs.

---

## Tasks

### Task 1: Create ShaPrint.Platform.Abstractions

**Goal:** Buat project interface murni (net8.0) dengan 7 interface + `ToastAction`, termasuk `IPrintRelayClient`.

**Files:**
- Create: `ShaPrint.Platform.Abstractions/ShaPrint.Platform.Abstractions.csproj`
- Create: `ShaPrint.Platform.Abstractions/IPrinterManager.cs`
- Create: `ShaPrint.Platform.Abstractions/IVirtualPrinterManager.cs`
- Create: `ShaPrint.Platform.Abstractions/IScannerService.cs`
- Create: `ShaPrint.Platform.Abstractions/IStartupManager.cs`
- Create: `ShaPrint.Platform.Abstractions/INotificationService.cs`
- Create: `ShaPrint.Platform.Abstractions/IFirewallManager.cs`
- Create: `ShaPrint.Platform.Abstractions/IPrintRelayClient.cs`
- Modify: `ShaPrint.sln` (add new project)

**Key interfaces (shape final, identik dengan spec):**

```csharp
// IVirtualPrinterManager.cs - TANPA pipeName
public interface IVirtualPrinterManager
{
    Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string driverName);
    Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string virtualPrinterName);
    bool CheckPrinterExists(string printerName);
    List<string> GetInstalledDrivers();
    List<string> GetInstalledVirtualPrinters();
}
```

```csharp
// IPrintRelayClient.cs
public interface IPrintRelayClient
{
    Task<bool> SendAsync(string targetPrinter, byte[] data, string documentName,
                         string? hostOverride = null, CancellationToken ct = default);
}
```

```csharp
// INotificationService.cs - record ToastAction sesuai kode existing
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

- [ ] **Step 1: Create ShaPrint.Platform.Abstractions.csproj** (net8.0, `ImplicitUsings`, `Nullable`, `ProjectReference` ke `ShaPrint.Core`)
- [ ] **Step 2: Create IPrinterManager.cs** (`GetLocalPrintersAsync`, `PrintRawDataAsync`)
- [ ] **Step 3: Create IVirtualPrinterManager.cs** (signature TANPA `pipeName`; ada `GetInstalledVirtualPrinters()`)
- [ ] **Step 4: Create IScannerService.cs** (`GetLocalScanners`, `PerformScan`)
- [ ] **Step 5: Create IStartupManager.cs** (`SetStartup`, `IsStartupEnabled`)
- [ ] **Step 6: Create INotificationService.cs** (`ToastAction` + 9 methods)
- [ ] **Step 7: Create IFirewallManager.cs** (`EnsureFirewallRulesAsync`)
- [ ] **Step 8: Create IPrintRelayClient.cs** (`SendAsync`)
- [ ] **Step 9: Add project to ShaPrint.sln**
- [ ] **Step 10: Build to verify compilation**

Run: `dotnet build ShaPrint.Platform.Abstractions/ShaPrint.Platform.Abstractions.csproj`
Expected: Build succeeded

- [ ] **Step 11: Commit**

```bash
git add ShaPrint.Platform.Abstractions/ ShaPrint.sln
git commit -m "feat: add ShaPrint.Platform.Abstractions with platform interfaces"
```

---

### Task 2: Extract ShaPrint.Platform.Windows (git mv + adapters, WpfApp still green)

**Goal:** Pindahkan service Windows existing via `git mv` (bukan salin) ke `ShaPrint.Platform.Windows`, tambah adapter yang mengimplementasikan interface Abstractions, dan pastikan `ShaPrint.WpfApp` tetap build + `ShaPrint.Tests` hijau. **WAJIB mempertahankan `DriverSafetyGuard` dan pipeline provisioning multi-vendor (Canon/HP/Brother) di jalur install virtual printer.**

**Files (git mv dari ShaPrint.WpfApp -> ShaPrint.Platform.Windows/Services/):**
- `ShaPrint.WpfApp/Services/Server/SpoolerApi.cs` -> `ShaPrint.Platform.Windows/Services/SpoolerApi.cs`
- `ShaPrint.WpfApp/Services/Server/ScannerService.cs` -> `ShaPrint.Platform.Windows/Services/ScannerService.cs`
- `ShaPrint.WpfApp/Services/Server/FirewallManager.cs` -> `ShaPrint.Platform.Windows/Services/FirewallManager.cs`
- `ShaPrint.WpfApp/Services/Client/VirtualPrinterManager.cs` -> `ShaPrint.Platform.Windows/Services/VirtualPrinterManager.cs`
- `ShaPrint.WpfApp/Services/Client/PipeListener.cs` -> `ShaPrint.Platform.Windows/Services/PipeListener.cs`
- `ShaPrint.WpfApp/Services/Client/DriverInstaller.cs` -> `ShaPrint.Platform.Windows/Services/DriverInstaller.cs`
- `ShaPrint.WpfApp/Services/Client/DriverPackageManager.cs` -> `ShaPrint.Platform.Windows/Services/DriverPackageManager.cs`
- `ShaPrint.WpfApp/Services/Client/DriverNameResolver.cs` -> `ShaPrint.Platform.Windows/Services/DriverNameResolver.cs`
- `ShaPrint.WpfApp/Services/Client/DriverPackageVerify.cs` -> `ShaPrint.Platform.Windows/Services/DriverPackageVerify.cs`
- `ShaPrint.WpfApp/Services/Client/SafeZipExtractor.cs` -> `ShaPrint.Platform.Windows/Services/SafeZipExtractor.cs`
- `ShaPrint.WpfApp/Services/Client/DriverSafetyGuard.cs` -> `ShaPrint.Platform.Windows/Services/DriverSafetyGuard.cs`
- `ShaPrint.WpfApp/Services/Client/RealProcessRunner.cs` -> `ShaPrint.Platform.Windows/Services/RealProcessRunner.cs`
- `ShaPrint.WpfApp/Services/INotificationService.cs` -> `ShaPrint.Platform.Windows/Services/INotificationService.cs`
- `ShaPrint.WpfApp/Services/NotificationService.cs` -> `ShaPrint.Platform.Windows/Services/NotificationService.cs`
- `ShaPrint.WpfApp/Utils/StartupManager.cs` -> `ShaPrint.Platform.Windows/Services/StartupManager.cs`

**Files (create adapters):**
- `ShaPrint.Platform.Windows/Adapters/WindowsPrinterManager.cs` (wraps `SpoolerApi`, implements `IPrinterManager`)
- `ShaPrint.Platform.Windows/Adapters/WindowsVirtualPrinterManager.cs` (wraps `VirtualPrinterManager`; derives pipe name internally; implements `IVirtualPrinterManager`)
- `ShaPrint.Platform.Windows/Adapters/WindowsScannerService.cs` (wraps `ScannerService`)
- `ShaPrint.Platform.Windows/Adapters/WindowsStartupManager.cs` (wraps `StartupManager`)
- `ShaPrint.Platform.Windows/Adapters/WindowsNotificationService.cs` (wraps `NotificationService`)
- `ShaPrint.Platform.Windows/Adapters/WindowsFirewallManager.cs` (wraps `FirewallManager`)
- `ShaPrint.Platform.Windows/Adapters/WindowsPrintRelayClient.cs` (implements `IPrintRelayClient` using `PipeListener`)

**Files (project + DI):**
- Create: `ShaPrint.Platform.Windows/ShaPrint.Platform.Windows.csproj` (net8.0-windows10.0.17763; references Abstractions + Core; `Microsoft.Toolkit.Uwp.Notifications`, `System.IO.Pipes.AccessControl`)
- Modify: `ShaPrint.WpfApp/ShaPrint.WpfApp.csproj` (add `ProjectReference` to `ShaPrint.Platform.Windows`)
- Modify: `ShaPrint.sln` (add new project)
- Modify: WpfApp DI registration + `using`/namespace directives (route through adapters; behavior unchanged)

**Notes on adapter fidelity:**
- `WindowsVirtualPrinterManager.InstallPrinterAsync(virtualPrinterName, driverName)` harus menurunkan `pipeName` secara internal (mis. `ShaPrint_{sanitized(virtualPrinterName)}`) lalu memanggil `VirtualPrinterManager` existing.
- Pipeline provisioning (`DriverInstaller`, `DriverPackageManager`, `DriverNameResolver`, `DriverPackageVerify`, `SafeZipExtractor`) dan `DriverSafetyGuard` TETAP dipanggil di jalur install (jangan dihapus/di-stub). Tujuan commit `2584399` (cegah BSOD/kernel corruption/spooler deadlock) dan `f31567d` (packaging & INF selection multi-vendor) harus tetap terpelihara.

- [ ] **Step 1: Create ShaPrint.Platform.Windows.csproj** (net8.0-windows10.0.17763, `EnableWindowsTargeting`, references Abstractions + Core, package refs)
- [ ] **Step 2: git mv server services** (SpoolerApi, ScannerService, FirewallManager)
- [ ] **Step 3: git mv client services** (VirtualPrinterManager, PipeListener, DriverInstaller, DriverPackageManager, DriverNameResolver, DriverPackageVerify, SafeZipExtractor, DriverSafetyGuard, RealProcessRunner)
- [ ] **Step 4: git mv notification + startup** (INotificationService, NotificationService, Utils/StartupManager)
- [ ] **Step 5: Update namespaces** on moved files (`ShaPrint.WpfApp.Services.*` -> `ShaPrint.Platform.Windows.*`) and internal `using` references; verify `DriverSafetyGuard` + provisioning call sites compile
- [ ] **Step 6: Create adapters** for all 7 interfaces (including `WindowsPrintRelayClient` for `IPrintRelayClient`)
- [ ] **Step 7: Wire DI in ShaPrint.WpfApp** to route through adapters (behavior unchanged)
- [ ] **Step 8: Add project to ShaPrint.sln**
- [ ] **Step 9: Build WpfApp and run tests**

Run:
```bash
dotnet build ShaPrint.WpfApp/ShaPrint.WpfApp.csproj
dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj
```
Expected: Build succeeded; all tests green (proves ekstraksi tidak mengubah perilaku Windows)

- [ ] **Step 10: Commit**

```bash
git add -A ShaPrint.Platform.Windows/ ShaPrint.WpfApp/ ShaPrint.sln
git commit -m "refactor: extract ShaPrint.Platform.Windows via git mv with adapters (DriverSafetyGuard + multi-vendor provisioning preserved)"
```

---

### Task 3: Create ShaPrint.UI Avalonia Skeleton (single Program.cs + runtime DI switch)

**Goal:** Buat skeleton `ShaPrint.UI` dengan SATU `Program.cs` (tanpa file entry point terpisah per-OS), DI switch runtime, dan `app.manifest` yang valid jika direferensikan.

**Files:**
- Create: `ShaPrint.UI/ShaPrint.UI.csproj`
- Create: `ShaPrint.UI/Program.cs`
- Create: `ShaPrint.UI/App.axaml`
- Create: `ShaPrint.UI/App.axaml.cs`
- Create: `ShaPrint.UI/app.manifest` (dibuat bersama csproj; TIDAK menunjuk file yang tidak ada)
- Modify: `ShaPrint.sln` (add new project)

**csproj key points:**
- `TargetFrameworks`: `net8.0;net8.0-windows` (TFM windows agar bisa mereferensikan `ShaPrint.Platform.Windows`; `Avalonia.Desktop` untuk Unix)
- `Avalonia` 11.2.x + `Avalonia.Desktop` + `Avalonia.Themes.Fluent` + `Avalonia.Fonts.Inter`
- `CommunityToolkit.Mvvm` 8.4.2 + `Microsoft.Extensions.Hosting` 10.0.8
- `ProjectReference` ke `ShaPrint.Core`, `ShaPrint.Platform.Abstractions`, dan (kondisional `net8.0-windows`) `ShaPrint.Platform.Windows`
- Jika memakai `<ApplicationManifest>app.manifest</ApplicationManifest>`, file `app.manifest` dibuat di task yang sama

**Program.cs key points:**
- SATU `[STAThread] static void Main(string[] args)`
- CLI branch `send` diproses dulu (lihat Task 8); untuk skeleton ini placeholder `CliDispatcher.TryHandle(args)` yang selalu `false`
- `AppBuilder.Configure<App>().UsePlatformDetect().WithInterFont().LogToTrace()`
- `ConfigureServices(IServiceCollection)` memilih platform RUNTIME: `OperatingSystem.IsWindows()` -> Windows services, `OperatingSystem.IsMacOS()/IsLinux()` -> Unix services
- `App.Host` dibangun TEPAT SATU kali

```csharp
// ShaPrint.UI/Program.cs (sketch)
public static class Program
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (CliDispatcher.TryHandle(args)) return; // `send` branch; Task 8

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
            services.AddPlatformWindows();
        else if (OperatingSystem.IsMacOS() || OperatingSystem.IsLinux())
            services.AddPlatformUnix();
    }
}
```

- [ ] **Step 1: Create ShaPrint.UI.csproj** (multi-TFM + packages + project refs)
- [ ] **Step 2: Create app.manifest** (valid, same task; if referenced)
- [ ] **Step 3: Create App.axaml** (`Application` + `FluentTheme`)
- [ ] **Step 4: Create App.axaml.cs** (build `App.Host` exactly once in `OnFrameworkInitializationCompleted`)
- [ ] **Step 5: Create Program.cs** (single entry point; runtime DI switch; CLI branch placeholder)
- [ ] **Step 6: Add project to ShaPrint.sln**
- [ ] **Step 7: Build both TFMs**

Run:
```bash
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0-windows
```
Expected: Both TFMs build succeeded

- [ ] **Step 8: Commit**

```bash
git add ShaPrint.UI/ ShaPrint.sln
git commit -m "feat: add ShaPrint.UI Avalonia skeleton with single Program.cs and runtime DI switch"
```

---

### Task 4: Migrate Shared Services (Discovery/Update/Monitor/PipeListener wiring)

**Goal:** Migrasi service yang bisa dibagi antar platform sebagai class injeksiabel via DI. Pilihan lokasi: `ShaPrint.UI/Services` untuk service yang bergantung pada networking `ShaPrint.Core` dan tidak menyentuh API OS (Discovery, Update, Monitor); service yang bergantung OS (PipeListener wiring Windows) tetap di `ShaPrint.Platform.Windows` dan diekspos via adapter. Ini menjaga `ShaPrint.UI` tetap bebas dari P/Invoke OS.

**Files:**
- Create: `ShaPrint.UI/Services/DiscoveryClientService.cs` (wraps/migrates `ShaPrint.WpfApp/Services/Client/DiscoveryClient.cs`)
- Create: `ShaPrint.UI/Services/DiscoveryServerService.cs` (wraps/migrates `ShaPrint.WpfApp/Services/Server/DiscoveryServer.cs`)
- Create: `ShaPrint.UI/Services/PrintReceiverService.cs` (wraps/migrates `ShaPrint.WpfApp/Services/Server/PrintReceiver.cs`)
- Create: `ShaPrint.UI/Services/MonitorService.cs` (wraps/migrates `ShaPrint.WpfApp/Services/Monitor/MonitorService.cs`)
- Create: `ShaPrint.UI/Services/UpdateService.cs` (wraps/migrates `ShaPrint.WpfApp/Services/UpdateService.cs`)
- Create: `ShaPrint.UI/Services/ScanClientService.cs` (wraps/migrates `ShaPrint.WpfApp/Services/Client/ScanClientService.cs`)
- Create: `ShaPrint.UI/Services/ServerReachabilityTracker.cs` (wraps/migrates `ShaPrint.WpfApp/Services/Client/ServerReachabilityTracker.cs`)
- Create: `ShaPrint.UI/Services/PrintRelayClientService.cs` (shared orchestration that resolves `IPrintRelayClient` via DI; used by CLI `send` + backend script path)
- Modify: `ShaPrint.UI/Program.cs` + DI extension methods (register the above)

**Notes:**
- `DiscoveryClient`, `DiscoveryServer`, `PrintReceiver`, `MonitorService`, `UpdateService`, `ScanClientService`, `ServerReachabilityTracker` dimigrasikan sebagai class injeksiabel (bukan static helper), agar bisa di-mock di test.
- `MonitorService` harus memakai `ServerStatusPayload` dari `ShaPrint.Core.Network` (bukan tipe monitor buatan yang tidak ada di codebase).
- TIDAK ada stub `Task.Delay` untuk fitur yang implementasinya sudah ada.

- [ ] **Step 1: Migrate DiscoveryClient + ServerReachabilityTracker** (UDP discovery + reachability)
- [ ] **Step 2: Migrate DiscoveryServer + PrintReceiver** (server-side)
- [ ] **Step 3: Migrate MonitorService** (bind to `ServerStatusPayload`)
- [ ] **Step 4: Migrate UpdateService + ScanClientService**
- [ ] **Step 5: Add PrintRelayClientService** (resolves `IPrintRelayClient`, wraps `SendAsync` with discovery + `PrintJobPayload`)
- [ ] **Step 6: Register services in DI** (Program.cs / extension)
- [ ] **Step 7: Build both TFMs**

Run:
```bash
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0-windows
```
Expected: Build succeeded

- [ ] **Step 8: Commit**

```bash
git add ShaPrint.UI/Services/
git commit -m "feat: migrate shared services (discovery, update, monitor, print receiver, scan client) to ShaPrint.UI"
```

---

### Task 5: Migrate Real ViewModels

**Goal:** Migrasi KODE NYATA dari `ShaPrint.WpfApp/ViewModels` ke `ShaPrint.UI/ViewModels` menggunakan services via DI. TIDAK boleh stub `Task.Delay` untuk fitur yang implementasinya sudah ada.

**Files (create, migrated real code):**
- `ShaPrint.UI/ViewModels/MainWindowViewModel.cs` (dari `ShaPrint.WpfApp/ViewModels/Windows/MainWindowViewModel.cs`)
- `ShaPrint.UI/ViewModels/Pages/ServerViewModel.cs`
- `ShaPrint.UI/ViewModels/Pages/ClientViewModel.cs`
- `ShaPrint.UI/ViewModels/Pages/MonitorViewModel.cs`
- `ShaPrint.UI/ViewModels/Pages/ScanViewModel.cs`
- `ShaPrint.UI/ViewModels/Pages/SettingsViewModel.cs`
- `ShaPrint.UI/ViewModels/Pages/UpdatesViewModel.cs`
- `ShaPrint.UI/ViewModels/Pages/WelcomeViewModel.cs`

**Rules:**
- Replace WPF-specific dependencies (`System.Windows`, `WPF-UI`) with `ShaPrint.Platform.Abstractions` + shared services from Task 4.
- `ScanLan` (Client), `CheckForUpdates` (Updates), `Start/StopServer` (Server) wajib memanggil service nyata, bukan `Task.Delay`.
- `MonitorViewModel` pakai `ServerStatusPayload`; binding hanya ke `HostName`, `Version`, `NetworkChannel`, `UptimeSeconds` (format via converter), `Printers.Count`, `ActiveClients.Count`.
- Seleksi printer/scanner TIDAK memakai properti seleksi; gunakan collection `SelectedItems` (lihat Task 6).
- Placeholder hanya untuk fitur yang memang ditunda, ditandai eksplisit `// deferred: ...`.

- [ ] **Step 1: Migrate MainWindowViewModel + WelcomeViewModel**
- [ ] **Step 2: Migrate ServerViewModel** (Start/Stop server via `DiscoveryServerService` + `PrintReceiverService` + `IFirewallManager`)
- [ ] **Step 3: Migrate ClientViewModel** (ScanLan via `DiscoveryClientService`; install/remove via `IVirtualPrinterManager` tanpa pipeName)
- [ ] **Step 4: Migrate MonitorViewModel** (bind to `ServerStatusPayload`, no custom monitor type)
- [ ] **Step 5: Migrate ScanViewModel + SettingsViewModel + UpdatesViewModel** (real update check via `UpdateService`)
- [ ] **Step 6: Register ViewModels in DI**
- [ ] **Step 7: Build both TFMs**

Run:
```bash
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0-windows
```
Expected: Build succeeded

- [ ] **Step 8: Commit**

```bash
git add ShaPrint.UI/ViewModels/
git commit -m "feat: migrate real ViewModels to ShaPrint.UI with DI-backed services"
```

---

### Task 6: Create Avalonia Views (correct compiled bindings)

**Goal:** Buat views Avalonia dengan binding yang valid terhadap `x:DataType` (compiled bindings), tanpa properti seleksi pada model Core.

**Files (create):**
- `ShaPrint.UI/Views/MainWindow.axaml` (+ `.axaml.cs`)
- `ShaPrint.UI/Views/Pages/ServerPage.axaml` (+ `.axaml.cs`)
- `ShaPrint.UI/Views/Pages/ClientPage.axaml` (+ `.axaml.cs`)
- `ShaPrint.UI/Views/Pages/MonitorPage.axaml` (+ `.axaml.cs`)
- `ShaPrint.UI/Views/Pages/ScanPage.axaml` (+ `.axaml.cs`)
- `ShaPrint.UI/Views/Pages/SettingsPage.axaml` (+ `.axaml.cs`)
- `ShaPrint.UI/Views/Pages/UpdatesPage.axaml` (+ `.axaml.cs`)
- `ShaPrint.UI/Views/Pages/WelcomePage.axaml` (+ `.axaml.cs`)

**Seleksi printer/scanner (tanpa properti seleksi pada model Core):**

```xml
<!-- ServerPage.axaml - printer selection via SelectionMode -->
<ListBox ItemsSource="{Binding AvailablePrinters}"
         SelectionMode="Multiple,Toggle"
         SelectedItems="{Binding SelectedPrinters}"
         x:DataType="vm:ServerViewModel">
    <ListBox.ItemTemplate>
        <DataTemplate x:DataType="network:PrinterInfo">
            <StackPanel Orientation="Horizontal" Spacing="10">
                <TextBlock Text="{Binding Name}" VerticalAlignment="Center"/>
                <TextBlock Text="{Binding DriverName}" VerticalAlignment="Center" Foreground="Gray"/>
            </StackPanel>
        </DataTemplate>
    </ListBox.ItemTemplate>
</ListBox>
```

**MonitorPage binding (hanya properti yang ada di `ServerStatusPayload`):**

```xml
<!-- MonitorPage.axaml - bind to real ServerStatusPayload properties -->
<TextBlock Text="{Binding HostName}" FontWeight="Bold" FontSize="16"/>
<TextBlock Text="{Binding Version}" Foreground="Gray"/>
<TextBlock Text="{Binding NetworkChannel}" Foreground="Gray"/>
<TextBlock Text="{Binding UptimeSeconds, Converter={x:Static conv:UptimeConverter.Instance}}" Foreground="Gray"/>
<TextBlock Text="{Binding Printers.Count, StringFormat='Printers: {0}'}"/>
<TextBlock Text="{Binding ActiveClients.Count, StringFormat='Clients: {0}'}"/>
```

- [ ] **Step 1: Create MainWindow.axaml** (sidebar navigation, `ContentControl` bound to `CurrentPage`)
- [ ] **Step 2: Create ServerPage.axaml** (printer/scanner `ListBox` dengan `SelectionMode="Multiple,Toggle"` + `SelectedItems`)
- [ ] **Step 3: Create ClientPage.axaml** (discovery, `ListBox SelectedItem` untuk target printer, installed printers list)
- [ ] **Step 4: Create MonitorPage.axaml** (bind ke `ServerStatusPayload`; converter `UptimeSeconds`)
- [ ] **Step 5: Create ScanPage.axaml, SettingsPage.axaml, UpdatesPage.axaml, WelcomePage.axaml**
- [ ] **Step 6: Add code-behind files** (`InitializeComponent()`)
- [ ] **Step 7: Build both TFMs** (verifikasi compiled bindings tidak error)

Run:
```bash
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0-windows
```
Expected: Build succeeded (compiled bindings resolve)

- [ ] **Step 8: Commit**

```bash
git add ShaPrint.UI/Views/
git commit -m "feat: create Avalonia views with correct compiled bindings and SelectionMode"
```

---

### Task 7: Implement ShaPrint.Platform.Unix Backends

**Goal:** Buat backend macOS+Linux dalam SATU project (net8.0) dengan guard runtime `OperatingSystem.IsMacOS()/IsLinux()`, CLI-first (tanpa P/Invoke ke shared library CUPS/SANE di v1).

**Files:**
- Create: `ShaPrint.Platform.Unix/ShaPrint.Platform.Unix.csproj` (net8.0; references Abstractions + Core)
- Create: `ShaPrint.Platform.Unix/UnixPrinterManager.cs`
- Create: `ShaPrint.Platform.Unix/UnixVirtualPrinterManager.cs`
- Create: `ShaPrint.Platform.Unix/UnixScannerService.cs`
- Create: `ShaPrint.Platform.Unix/UnixStartupManager.cs`
- Create: `ShaPrint.Platform.Unix/UnixNotificationService.cs`
- Create: `ShaPrint.Platform.Unix/UnixFirewallManager.cs`
- Create: `ShaPrint.Platform.Unix/UnixPrintRelayClient.cs` (shared relay logic; resolves server via DiscoveryClient)
- Create: `ShaPrint.Platform.Unix/PrivilegeEscalationHelper.cs` (detect write failure -> sudo/pkexec instructions)

**Key implementation rules:**
- `UnixPrinterManager`: `lpstat -p` (enumerasi), `lp -d <name> -t <title> <file>` (print). TIDAK pakai `IntPtr.Size * 3` / `cups_dest_t` marshaling.
- `UnixScannerService`: `scanimage -L` (list) dan `scanimage -d <name> --format=<fmt> --mode=<mode> --resolution=<dpi>` dengan output dibaca via `RedirectStandardOutput` lalu ditulis ke file oleh C# (DILARANG `>` di `Arguments`).
- `UnixVirtualPrinterManager`: tulis backend script + PPD + `lpadmin`. `GetInstalledVirtualPrinters()` filter prefix nama.
- **Privilege**: menulis `/usr/lib/cups/backend` dan `/usr/share/cups/model` butuh root. Deteksi kegagalan tulis (`UnauthorizedAccessException`/`IOException`) -> tampilkan perintah `sudo`/`pkexec` yang harus dijalankan user (atau `postinst` saat packaging). DILARANG menganggap `File.WriteAllText` biasa berhasil.

**Backend script (referensi, disimpan sebagai string/asset):**

```bash
#!/bin/bash
# /usr/lib/cups/backend/shaprint
PRINTER="$6"
TMP=$(mktemp /tmp/shaprint_job_XXXXXX)
cat > "$TMP"
shaprint send --printer "$PRINTER" --file "$TMP"
rc=$?
rm -f "$TMP"
exit $rc
```

- [ ] **Step 1: Create ShaPrint.Platform.Unix.csproj**
- [ ] **Step 2: Implement UnixPrinterManager** (lpstat/lp, runtime guard MacOS/Linux)
- [ ] **Step 3: Implement UnixVirtualPrinterManager** (backend script + PPD + lpadmin + `GetInstalledVirtualPrinters`)
- [ ] **Step 4: Implement PrivilegeEscalationHelper** (detect failure -> sudo/pkexec instruction; no fake success)
- [ ] **Step 5: Implement UnixScannerService** (scanimage via `RedirectStandardOutput`, no `>` in Arguments)
- [ ] **Step 6: Implement UnixStartupManager** (LaunchAgent macOS / systemd user Linux)
- [ ] **Step 7: Implement UnixNotificationService** (osascript / notify-send)
- [ ] **Step 8: Implement UnixFirewallManager** (pfctl / ufw / firewall-cmd / iptables, best-effort)
- [ ] **Step 9: Implement UnixPrintRelayClient** (IPrintRelayClient + DiscoveryClient + PrintJobPayload)
- [ ] **Step 10: Build**

Run:
```bash
dotnet build ShaPrint.Platform.Unix/ShaPrint.Platform.Unix.csproj
```
Expected: Build succeeded

- [ ] **Step 11: Commit**

```bash
git add ShaPrint.Platform.Unix/
git commit -m "feat: implement CLI-first ShaPrint.Platform.Unix backends (CUPS, SANE scanimage, privilege escalation)"
```

---

### Task 8: Implement CLI Sender `send` + Backend Script Wiring

**Goal:** Tutup lubang fungsional client print path Unix. Verb `send` diimplementasikan di `Program.cs` `ShaPrint.UI` sebagai branch CLI SEBELUM start GUI. Backend CUPS memanggil CLI sender; tidak ada lagi pola `cat > /tmp ... rm` yang membuang data.

**Files:**
- Create: `ShaPrint.UI/Cli/CliDispatcher.cs` (parses `shaprint send --printer <name> --file <path> [--host <ip>]`)
- Create: `ShaPrint.UI/Cli/SendCommand.cs` (reads file, calls `IPrintRelayClient.SendAsync`)
- Modify: `ShaPrint.UI/Program.cs` (wire `CliDispatcher.TryHandle(args)` before GUI)
- Modify: `ShaPrint.UI/Services/PrintRelayClientService.cs` (reuse for CLI path)
- Modify: `ShaPrint.Platform.Unix/UnixVirtualPrinterManager.cs` (emit backend script that invokes `shaprint send`, not the discard pattern)

**Flow (wajib):**
```
CUPS backend script -> write job to temp file
  -> `shaprint send --printer <name> --file <path> [--host <ip>]`
  -> IPrintRelayClient + DiscoveryClient + PrintJobPayload (AES-256-GCM from ShaPrint.Core)
  -> TCP 9877 -> server
  -> temp file deleted
```

- [ ] **Step 1: Create CliDispatcher.cs** (detect `send` verb, `--printer`, `--file`, `--host`)
- [ ] **Step 2: Create SendCommand.cs** (read file bytes, resolve `IPrintRelayClient`, call `SendAsync`)
- [ ] **Step 3: Wire CLI branch in Program.cs** (before `StartWithClassicDesktopLifetime`)
- [ ] **Step 4: Update UnixVirtualPrinterManager** to emit backend script calling `shaprint send` (no `rm`-only discard)
- [ ] **Step 5: Build both TFMs**

Run:
```bash
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0-windows
dotnet build ShaPrint.Platform.Unix/ShaPrint.Platform.Unix.csproj
```
Expected: Build succeeded

- [ ] **Step 6: Smoke test CLI parsing** (no server needed)

Run: `dotnet run --project ShaPrint.UI/ShaPrint.UI.csproj -f net8.0 -- send --printer demo --file /tmp/job.bin`
Expected: CLI branch executes `SendAsync` (may fail to connect, but parses and attempts send; no GUI launch)

- [ ] **Step 7: Commit**

```bash
git add ShaPrint.UI/Cli/ ShaPrint.UI/Program.cs ShaPrint.Platform.Unix/
git commit -m "feat: add shaprint send CLI verb and wire CUPS backend script"
```

---

### Task 9: Create ShaPrint.Android Project

**Goal:** Buat project Android TERPISAH (net8.0-android), bukan folder dalam `ShaPrint.UI`.

**Files:**
- Create: `ShaPrint.Android/ShaPrint.Android.csproj` (net8.0-android; `Avalonia.Android`; minSdk 21, targetSdk 34, versionName 2.0.0)
- Create: `ShaPrint.Android/MainActivity.cs` (`: AvaloniaMainActivity`)
- Create: `ShaPrint.Android/AndroidManifest.xml`
- Create: `ShaPrint.Android/Services/DiscoveryService.cs` (UDP + `WifiManager.MulticastLock`)
- Create: `ShaPrint.Android/Services/PrintRelayService.cs` (IPrintRelayClient path)
- Create: `ShaPrint.Android/Resources/` (styles, icons)
- Modify: `ShaPrint.sln` (add project)

**MainActivity key points:**
- Host DI dibangun SEKALI. `MainActivity` men-set konfigurasi layanan Android via static callback/delegate SEBELUM Avalonia memanggil `OnFrameworkInitializationCompleted` (tidak ada race dua `Host`).
- UDP discovery: `WifiManager.MulticastLock` acquire saat scan, release setelah selesai.
- File picker: Storage Access Framework (`ACTION_OPEN_DOCUMENT`).

**AndroidManifest.xml permissions (HANYA ini):**
```xml
<uses-permission android:name="android.permission.INTERNET" />
<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
<uses-permission android:name="android.permission.ACCESS_WIFI_STATE" />
<uses-permission android:name="android.permission.CHANGE_WIFI_MULTICAST_STATE" />
<uses-permission android:name="android.permission.POST_NOTIFICATIONS" />
```

- [ ] **Step 1: Create ShaPrint.Android.csproj** (net8.0-android, minSdk 21, targetSdk 34, versionName 2.0.0, Avalonia.Android)
- [ ] **Step 2: Create AndroidManifest.xml** (only the 5 allowed permissions; no external-storage permissions)
- [ ] **Step 3: Create MainActivity.cs** (static callback sets Android services before `OnFrameworkInitializationCompleted`)
- [ ] **Step 4: Implement DiscoveryService** (UDP discovery + `WifiManager.MulticastLock` acquire/release)
- [ ] **Step 5: Implement PrintRelayService** (file -> `IPrintRelayClient.SendAsync` via TCP 9877)
- [ ] **Step 6: Add Resources** (styles + icon)
- [ ] **Step 7: Add project to ShaPrint.sln**
- [ ] **Step 8: Build Android TFM**

Run:
```bash
dotnet build ShaPrint.Android/ShaPrint.Android.csproj -f net8.0-android
```
Expected: Build succeeded

- [ ] **Step 9: Commit**

```bash
git add ShaPrint.Android/ ShaPrint.sln
git commit -m "feat: add ShaPrint.Android project with multicast discovery and SAF file picker"
```

---

### Task 10: CI Workflow Matrix + Contract Tests

**Goal:** Tambah CI matrix dan contract tests platform di `ShaPrint.Tests`.

**Files:**
- Create: `.github/workflows/ci.yml` (matrix: windows-latest, ubuntu-latest, macos-latest)
- Create: `ShaPrint.Tests/Contract/IPrinterManagerContractTests.cs`
- Create: `ShaPrint.Tests/Contract/IVirtualPrinterManagerContractTests.cs`
- Create: `ShaPrint.Tests/Contract/IPrintRelayClientContractTests.cs`
- Create: `ShaPrint.Tests/Contract/ServerStatusPayloadTests.cs`
- Modify: `ShaPrint.Tests/ShaPrint.Tests.csproj` (reference Abstractions/Platform projects)

**CI matrix steps:**
- `dotnet build ShaPrint.sln` (pada runner yang relevan; Windows build WpfApp; Unix build Platform.Unix)
- `dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj`
- `dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0` (ubuntu/macos) dan `-f net8.0-windows` (windows)
- `dotnet build ShaPrint.Android/ShaPrint.Android.csproj -f net8.0-android` (pada salah satu runner Linux/Windows)

**Contract tests (os-agnostic):**
- `IVirtualPrinterManager` signature TIDAK memuat `pipeName` (compiler-enforced saat implementasi dikompilasi).
- `IPrintRelayClient.SendAsync` memvalidasi `PrintJobPayload` (serialisasi ukuran target printer + spool data sesuai `Constants`).
- `ServerStatusPayload` shape (`HostName`, `Version`, `NetworkChannel`, `UptimeSeconds`, `Printers`, `ActiveClients`) konsisten untuk binding Monitor.

- [ ] **Step 1: Create CI workflow** with 3-runner matrix
- [ ] **Step 2: Add contract tests for IVirtualPrinterManager (no pipeName)**
- [ ] **Step 3: Add contract tests for IPrintRelayClient (PrintJobPayload validation)**
- [ ] **Step 4: Add contract tests for ServerStatusPayload shape**
- [ ] **Step 5: Reference platform projects from ShaPrint.Tests**
- [ ] **Step 6: Run tests locally**

Run:
```bash
dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj
```
Expected: All tests green

- [ ] **Step 7: Commit**

```bash
git add .github/workflows/ ShaPrint.Tests/
git commit -m "ci: add multi-OS build matrix and platform contract tests"
```

---

### Task 11: Documentation (multi-platform-guide + README)

**Goal:** Dokumentasi konsisten dengan keputusan desain final (macOS scanner = SANE brew, kebutuhan root, CLI sender).

**Files:**
- Create: `docs/multi-platform-guide.md`
- Modify: `README.md`

**multi-platform-guide wajib mencakup:**
- Tabel platform support (Server/Client/Monitor/Print-only).
- Build dari source: `dotnet build`/`publish` per platform; Android `-f net8.0-android`.
- macOS scanner = SANE: `brew install sane-backends`; verifikasi `scanimage -L`.
- Linux scanner = SANE: `sudo apt install sane-utils` (atau distro equivalent).
- Kebutuhan root untuk install virtual printer (backend + PPD): instruksi `sudo`/`pkexec`/postinst.
- CLI sender: `shaprint send --printer <name> --file <path> [--host <ip>]`.
- Troubleshooting (multicast discovery Android, firewall, network channel).

**README update:**
- Tambah section Multi-Platform Support (Windows/macOS/Linux/Android) dengan link ke guide.

- [ ] **Step 1: Create docs/multi-platform-guide.md** (SANE brew, root requirements, CLI sender, troubleshooting)
- [ ] **Step 2: Update README.md** (multi-platform section + link)
- [ ] **Step 3: Review doc consistency** against spec (Decisions + Risks + Future Work)

Run: `git diff --stat docs/ README.md`
Expected: Only docs changed

- [ ] **Step 4: Commit**

```bash
git add docs/multi-platform-guide.md README.md
git commit -m "docs: add multi-platform guide and update README"
```

---

## Summary

Plan ini memigrasikan ShaPrint dari Windows-only ke multi-platform dalam 4 fase:

1. **Fase 1 (Task 1-2)**: `ShaPrint.Platform.Abstractions` + ekstraksi `ShaPrint.Platform.Windows` via `git mv` (dengan adapter), `DriverSafetyGuard` + provisioning multi-vendor dipertahankan, `ShaPrint.WpfApp` tetap build & test hijau.
2. **Fase 2 (Task 3-8)**: `ShaPrint.UI` (Avalonia, satu `Program.cs`, DI runtime), migrasi shared services + ViewModels nyata + Views dengan binding benar, `ShaPrint.Platform.Unix` CLI-first (CUPS + SANE + privilege escalation), CLI sender `send`.
3. **Fase 3 (Task 9)**: `ShaPrint.Android` terpisah (multicast lock, SAF file picker).
4. **Fase 4 (Task 10-11)**: CI matrix (windows/ubuntu/macos) + contract tests + docs.

Total: 11 tasks. Estimasi realistis: Fase 1 (1-2 minggu), Fase 2 (3-5 minggu), Fase 3 (2-3 minggu), Fase 4 (1-2 minggu) -> keseluruhan ~7-12 minggu, bergantung ketersediaan device fisik untuk macOS/Android dan CI matrix.
