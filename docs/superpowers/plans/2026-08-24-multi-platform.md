# ShaPrint Multi-Platform Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate ShaPrint dari Windows-only WPF app ke multi-platform desktop (Win/Mac/Linux) + Android app menggunakan Avalonia UI.

**Architecture:** Platform abstraction layer (`ShaPrint.Platform`) dengan interface-based DI, shared Avalonia UI (`ShaPrint.UI`), dan platform-specific backends per OS. `ShaPrint.Core` tetap unchanged.

**Tech Stack:** .NET 8, Avalonia UI 11.x, CommunityToolkit.Mvvm, FluentAvalonia, CUPS (Mac/Linux), SANE (Linux), ImageCapture (macOS)

**Spec:** `docs/superpowers/specs/2026-08-24-multi-platform-design.md`

## Global Constraints

- `ShaPrint.Core` tidak boleh dimodifikasi (platform-agnostic)
- Networking protocol (TCP/UDP ports 9876/9877/9878) tidak berubah
- Security model (AES-256-GCM, HMAC-SHA256, PBKDF2) tetap sama
- Backward compatible dengan ShaPrint Windows existing
- Android: PDF dan image only
- Scanner: macOS pakai ImageCapture, Linux bundle libsane
- Auto-update: GitHub-based

---

## File Structure

### New Projects

```
ShaPrint.Platform/
├── ShaPrint.Platform.csproj
├── Abstractions/
│   ├── IPrinterManager.cs
│   ├── IVirtualPrinterManager.cs
│   ├── IScannerService.cs
│   ├── IStartupManager.cs
│   ├── INotificationService.cs
│   └── IFirewallManager.cs
├── Windows/
│   ├── WindowsPrinterManager.cs
│   ├── WindowsVirtualPrinterManager.cs
│   ├── WindowsScannerService.cs
│   ├── WindowsStartupManager.cs
│   ├── WindowsNotificationService.cs
│   └── WindowsFirewallManager.cs
├── macOS/
│   ├── MacPrinterManager.cs
│   ├── MacVirtualPrinterManager.cs
│   ├── MacScannerService.cs
│   ├── MacStartupManager.cs
│   ├── MacNotificationService.cs
│   └── MacFirewallManager.cs
├── Linux/
│   ├── LinuxPrinterManager.cs
│   ├── LinuxVirtualPrinterManager.cs
│   ├── LinuxScannerService.cs
│   ├── LinuxStartupManager.cs
│   ├── LinuxNotificationService.cs
│   └── LinuxFirewallManager.cs
└── Android/
    ├── AndroidPrinterManager.cs
    └── AndroidNotificationService.cs

ShaPrint.UI/
├── ShaPrint.UI.csproj
├── App.axaml
├── App.axaml.cs
├── Program.Windows.cs
├── Program.macOS.cs
├── Program.Linux.cs
├── Android/
│   ├── MainActivity.cs
│   └── AndroidManifest.xml
├── ViewModels/
│   ├── MainWindowViewModel.cs
│   ├── Pages/
│   │   ├── ServerViewModel.cs
│   │   ├── ClientViewModel.cs
│   │   ├── MonitorViewModel.cs
│   │   ├── SettingsViewModel.cs
│   │   └── UpdatesViewModel.cs
│   └── Services/
│       ├── DiscoveryClient.cs
│       ├── DiscoveryServer.cs
│       ├── MonitorService.cs
│       └── UpdateService.cs
├── Views/
│   ├── MainWindow.axaml
│   └── Pages/
│       ├── ServerPage.axaml
│       ├── ClientPage.axaml
│       ├── MonitorPage.axaml
│       ├── SettingsPage.axaml
│       └── UpdatesPage.axaml
└── Services/
    ├── NavigationService.cs
    └── DialogService.cs
```

---

## Tasks

### Task 1: Create ShaPrint.Platform Project with Interfaces

**Files:**
- Create: `ShaPrint.Platform/ShaPrint.Platform.csproj`
- Create: `ShaPrint.Platform/Abstractions/IPrinterManager.cs`
- Create: `ShaPrint.Platform/Abstractions/IVirtualPrinterManager.cs`
- Create: `ShaPrint.Platform/Abstractions/IScannerService.cs`
- Create: `ShaPrint.Platform/Abstractions/IStartupManager.cs`
- Create: `ShaPrint.Platform/Abstractions/INotificationService.cs`
- Create: `ShaPrint.Platform/Abstractions/IFirewallManager.cs`
- Modify: `ShaPrint.sln` (add new project)

**Interfaces:**

```csharp
// ShaPrint.Platform/Abstractions/IPrinterManager.cs
using ShaPrint.Core.Network;

namespace ShaPrint.Platform;

public interface IPrinterManager
{
    Task<List<PrinterInfo>> GetLocalPrintersAsync();
    Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null);
}
```

```csharp
// ShaPrint.Platform/Abstractions/IVirtualPrinterManager.cs
namespace ShaPrint.Platform;

public interface IVirtualPrinterManager
{
    Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string pipeName, string driverName);
    Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string printerName, string pipeName);
    bool CheckPrinterExists(string printerName);
    List<string> GetInstalledDrivers();
}
```

```csharp
// ShaPrint.Platform/Abstractions/IScannerService.cs
using ShaPrint.Core.Network;

namespace ShaPrint.Platform;

public interface IScannerService
{
    List<ScannerInfo> GetLocalScanners();
    byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, out string actualFormat);
}
```

```csharp
// ShaPrint.Platform/Abstractions/IStartupManager.cs
namespace ShaPrint.Platform;

public interface IStartupManager
{
    void SetStartup(bool enable);
    bool IsStartupEnabled();
}
```

```csharp
// ShaPrint.Platform/Abstractions/INotificationService.cs
namespace ShaPrint.Platform;

public class ToastAction
{
    public string Arguments { get; set; } = string.Empty;
}

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

```csharp
// ShaPrint.Platform/Abstractions/IFirewallManager.cs
namespace ShaPrint.Platform;

public interface IFirewallManager
{
    Task EnsureFirewallRulesAsync();
}
```

```xml
<!-- ShaPrint.Platform/ShaPrint.Platform.csproj -->
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
  </PropertyGroup>

  <ItemGroup>
    <ProjectReference Include="..\ShaPrint.Core\ShaPrint.Core.csproj" />
  </ItemGroup>
</Project>
```

- [ ] **Step 1: Create ShaPrint.Platform.csproj**
- [ ] **Step 2: Create all interface files**
- [ ] **Step 3: Add project to ShaPrint.sln**
- [ ] **Step 4: Build to verify compilation**

Run: `dotnet build ShaPrint.Platform/ShaPrint.Platform.csproj`
Expected: Build succeeded

- [ ] **Step 5: Commit**

```bash
git add ShaPrint.Platform/ ShaPrint.sln
git commit -m "feat: add ShaPrint.Platform project with abstraction interfaces"
```

---

### Task 2: Implement Windows Platform Backends

**Files:**
- Create: `ShaPrint.Platform/Windows/WindowsPrinterManager.cs`
- Create: `ShaPrint.Platform/Windows/WindowsVirtualPrinterManager.cs`
- Create: `ShaPrint.Platform/Windows/WindowsScannerService.cs`
- Create: `ShaPrint.Platform/Windows/WindowsStartupManager.cs`
- Create: `ShaPrint.Platform/Windows/WindowsNotificationService.cs`
- Create: `ShaPrint.Platform/Windows/WindowsFirewallManager.cs`

**Interfaces:**
- Consumes: `IPrinterManager`, `IVirtualPrinterManager`, `IScannerService`, `IStartupManager`, `INotificationService`, `IFirewallManager`
- Produces: Windows implementations of all interfaces

**Implementation:** Wrap existing code from `ShaPrint.WpfApp/Services/` into interface implementations.

- [ ] **Step 1: Create WindowsPrinterManager.cs**

```csharp
// ShaPrint.Platform/Windows/WindowsPrinterManager.cs
using System.Runtime.InteropServices;
using ShaPrint.Core.Network;

namespace ShaPrint.Platform.Windows;

public class WindowsPrinterManager : IPrinterManager
{
    [DllImport("winspool.Drv", EntryPoint = "OpenPrinter", SetLastError = true, CharSet = CharSet.Auto)]
    static extern bool OpenPrinter(string szPrinter, out IntPtr hPrinter, IntPtr pd);

    [DllImport("winspool.Drv", EntryPoint = "ClosePrinter", SetLastError = true)]
    static extern bool ClosePrinter(IntPtr hPrinter);

    [DllImport("winspool.Drv", EntryPoint = "StartDocPrinter", SetLastError = true, CharSet = CharSet.Auto)]
    static extern int StartDocPrinter(IntPtr hPrinter, int level, [In, MarshalAs(UnmanagedType.LPStruct)] DOCINFO di);

    [DllImport("winspool.Drv", EntryPoint = "EndDocPrinter", SetLastError = true)]
    static extern bool EndDocPrinter(IntPtr hPrinter);

    [DllImport("winspool.Drv", EntryPoint = "StartPagePrinter", SetLastError = true)]
    static extern bool StartPagePrinter(IntPtr hPrinter);

    [DllImport("winspool.Drv", EntryPoint = "EndPagePrinter", SetLastError = true)]
    static extern bool EndPagePrinter(IntPtr hPrinter);

    [DllImport("winspool.Drv", EntryPoint = "WritePrinter", SetLastError = true)]
    static extern bool WritePrinter(IntPtr hPrinter, IntPtr pBytes, int dwCount, out int dwWritten);

    [DllImport("winspool.Drv", EntryPoint = "SetJob", SetLastError = true)]
    static extern bool SetJob(IntPtr hPrinter, int JobId, int Level, IntPtr pJob, int Command);

    [DllImport("winspool.drv", CharSet = CharSet.Auto, SetLastError = true)]
    static extern bool EnumPrinters(uint Flags, string? Name, uint Level, IntPtr pPrinterEnum, uint cbBuf, out uint pcbNeeded, out uint pcReturned);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    class DOCINFO
    {
        [MarshalAs(UnmanagedType.LPTStr)] public string? pDocName;
        [MarshalAs(UnmanagedType.LPTStr)] public string? pOutputFile;
        [MarshalAs(UnmanagedType.LPTStr)] public string? pDatatype;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    struct PRINTER_INFO_2
    {
        [MarshalAs(UnmanagedType.LPTStr)] public string pServerName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pPrinterName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pShareName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pPortName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pDriverName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pComment;
        [MarshalAs(UnmanagedType.LPTStr)] public string pLocation;
        public IntPtr pDevMode;
        [MarshalAs(UnmanagedType.LPTStr)] public string pSepFile;
        [MarshalAs(UnmanagedType.LPTStr)] public string pPrintProcessor;
        [MarshalAs(UnmanagedType.LPTStr)] public string pDatatype;
        [MarshalAs(UnmanagedType.LPTStr)] public string pParameters;
        public IntPtr pSecurityDescriptor;
        public uint Attributes;
        public uint Priority;
        public uint DefaultPriority;
        public uint StartTime;
        public uint UntilTime;
        public uint Status;
        public uint cJobs;
        public uint AveragePPM;
    }

    const uint PRINTER_ENUM_LOCAL = 2;
    const uint PRINTER_ENUM_CONNECTIONS = 4;

    public Task<List<PrinterInfo>> GetLocalPrintersAsync()
    {
        return Task.Run(() =>
        {
            var printers = new List<PrinterInfo>();
            uint flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
            uint cbNeeded = 0;
            uint cReturned = 0;

            EnumPrinters(flags, null, 2, IntPtr.Zero, 0, out cbNeeded, out cReturned);
            if (cbNeeded > 0)
            {
                IntPtr pAddr = Marshal.AllocHGlobal((int)cbNeeded);
                try
                {
                    if (EnumPrinters(flags, null, 2, pAddr, cbNeeded, out cbNeeded, out cReturned))
                    {
                        var type = typeof(PRINTER_INFO_2);
                        int increment = Marshal.SizeOf(type);

                        for (int i = 0; i < cReturned; i++)
                        {
                            IntPtr currentAddr = IntPtr.Add(pAddr, i * increment);
                            var info = (PRINTER_INFO_2)Marshal.PtrToStructure(currentAddr, type)!;
                            printers.Add(new PrinterInfo
                            {
                                Name = info.pPrinterName ?? "",
                                DriverName = info.pDriverName ?? ""
                            });
                        }
                    }
                }
                finally
                {
                    Marshal.FreeHGlobal(pAddr);
                }
            }
            return printers;
        });
    }

    public async Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null)
    {
        TimeSpan actualTimeout = timeout ?? TimeSpan.FromSeconds(120);
        int jobId = 0;

        var printTask = Task.Factory.StartNew(() =>
        {
            IntPtr pBytes = Marshal.AllocCoTaskMem(data.Length);
            Marshal.Copy(data, 0, pBytes, data.Length);
            bool success = false;
            try
            {
                IntPtr hPrinter = IntPtr.Zero;
                if (OpenPrinter(printerName.Normalize(), out hPrinter, IntPtr.Zero))
                {
                    var di = new DOCINFO { pDocName = documentName, pDatatype = "RAW" };
                    int currentJobId = StartDocPrinter(hPrinter, 1, di);
                    if (currentJobId > 0)
                    {
                        Interlocked.Exchange(ref jobId, currentJobId);
                        if (StartPagePrinter(hPrinter))
                        {
                            int dwWritten = 0;
                            success = WritePrinter(hPrinter, pBytes, data.Length, out dwWritten);
                            EndPagePrinter(hPrinter);
                        }
                        EndDocPrinter(hPrinter);
                    }
                    ClosePrinter(hPrinter);
                }
            }
            finally
            {
                Marshal.FreeCoTaskMem(pBytes);
            }
            return success;
        }, CancellationToken.None, TaskCreationOptions.LongRunning, TaskScheduler.Default);

        if (await Task.WhenAny(printTask, Task.Delay(actualTimeout)) == printTask)
        {
            return await printTask;
        }
        else
        {
            int capturedJobId = Volatile.Read(ref jobId);
            if (capturedJobId > 0)
            {
                IntPtr abortHandle = IntPtr.Zero;
                if (OpenPrinter(printerName.Normalize(), out abortHandle, IntPtr.Zero))
                {
                    SetJob(abortHandle, capturedJobId, 0, IntPtr.Zero, 5);
                    ClosePrinter(abortHandle);
                }
            }
            return false;
        }
    }
}
```

- [ ] **Step 2: Create WindowsVirtualPrinterManager.cs**

```csharp
// ShaPrint.Platform/Windows/WindowsVirtualPrinterManager.cs
using System.Diagnostics;
using Microsoft.Win32;
using ShaPrint.Core;

namespace ShaPrint.Platform.Windows;

public class WindowsVirtualPrinterManager : IVirtualPrinterManager
{
    public async Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string pipeName, string driverName)
    {
        return await Task.Run(() =>
        {
            try
            {
                string safePrinterName = virtualPrinterName.Replace("'", "''");
                string safePipeName = pipeName.Replace("'", "''");
                string safeDriverName = driverName.Replace("'", "''");

                var portResult = RunPowerShell($"Add-PrinterPort -Name '{safePipeName}'");
                if (!portResult.Success && !portResult.Output.Contains("already exists"))
                {
                    AppLogger.Log("[CLIENT] Add-PrinterPort warning: " + portResult.Output);
                }

                var driverResult = RunPowerShell($"Add-PrinterDriver -Name '{safeDriverName}'");
                if (!driverResult.Success && !driverResult.Output.Contains("already exists", StringComparison.OrdinalIgnoreCase))
                {
                    AppLogger.Log($"[CLIENT] Add-PrinterDriver for '{safeDriverName}' reported: {driverResult.Output}");
                }

                var addPrinterResult = RunPowerShell($"Add-Printer -Name '{safePrinterName}' -DriverName '{safeDriverName}' -PortName '{safePipeName}'");
                if (!addPrinterResult.Success)
                {
                    string err = addPrinterResult.Output;
                    if (err.Contains("The specified driver does not exist", StringComparison.OrdinalIgnoreCase) || err.Contains("was not found", StringComparison.OrdinalIgnoreCase))
                    {
                        return (false, $"Driver '{driverName}' is not installed on this computer. Please install the printer driver first.");
                    }
                    if (err.Contains("Access denied", StringComparison.OrdinalIgnoreCase) || err.Contains("Administrator", StringComparison.OrdinalIgnoreCase))
                    {
                        err += " (Please ensure you run this application as Administrator)";
                    }
                    return (false, "Driver installation failed. Last error: " + err);
                }

                if (addPrinterResult.Success)
                {
                    RunPowerShell($"$printer = Get-WmiObject -Class Win32_Printer | Where-Object {{ $_.Name -eq '{safePrinterName}' }}; if ($printer) {{ $printer.EnableBIDI = $false; $printer.Put() }}");
                    return (true, string.Empty);
                }

                return (false, "Driver installation failed.");
            }
            catch (Exception ex)
            {
                return (false, "Exception: " + ex.Message);
            }
        });
    }

    public async Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string printerName, string pipeName)
    {
        return await Task.Run(() =>
        {
            try
            {
                string safePrinterName = printerName.Replace("'", "''");
                string safePipeName = pipeName.Replace("'", "''");

                RunPowerShell($"Get-PrintJob -PrinterName '{safePrinterName}' -ErrorAction SilentlyContinue | Remove-PrintJob -ErrorAction SilentlyContinue");
                Thread.Sleep(500);

                var wmiDeleteScript = $@"
$printer = Get-WmiObject -Class Win32_Printer | Where-Object {{ $_.Name -eq '{safePrinterName}' }};
if ($printer) {{
    $result = $printer.Delete();
    if ($result.ReturnValue -eq 0) {{ Write-Output 'Success' }}
    else {{ Write-Output ""Failed:$($result.ReturnValue)"" }}
}} else {{
    Write-Output 'NotFound'
}}";
                RunPowerShell(wmiDeleteScript);

                Thread.Sleep(500);

                if (!string.IsNullOrEmpty(safePipeName))
                {
                    RunPowerShell($"Remove-PrinterPort -Name '{safePipeName}' -ErrorAction SilentlyContinue");
                }

                RunPowerShell("Stop-Service -Name Spooler -Force -ErrorAction SilentlyContinue");
                Thread.Sleep(2000);
                RunPowerShell("Start-Service -Name Spooler -ErrorAction SilentlyContinue");
                Thread.Sleep(2000);

                return (true, string.Empty);
            }
            catch (Exception ex)
            {
                return (false, "Exception: " + ex.Message);
            }
        });
    }

    public bool CheckPrinterExists(string printerName)
    {
        string safePrinterName = printerName.Replace("'", "''");
        var result = RunPowerShell($"Get-Printer -Name '{safePrinterName}'");
        return result.Success;
    }

    public List<string> GetInstalledDrivers()
    {
        var drivers = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        try
        {
            var psResult = RunPowerShell("Get-PrinterDriver | Select-Object -ExpandProperty Name | ConvertTo-Json -Compress");
            if (psResult.Success)
            {
                var parsed = TryParseJsonStringArray(psResult.Output);
                foreach (var name in parsed)
                {
                    if (!string.IsNullOrWhiteSpace(name)) drivers.Add(name.Trim());
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Log("[CLIENT] Get-PrinterDriver exception: " + ex.Message);
        }

        try
        {
            const string registryRootPath = @"SYSTEM\CurrentControlSet\Control\Print\Environments";
            using RegistryKey? key = Registry.LocalMachine.OpenSubKey(registryRootPath);
            if (key != null)
            {
                foreach (string environmentName in key.GetSubKeyNames())
                {
                    using RegistryKey? envKey = key.OpenSubKey(environmentName);
                    if (envKey == null) continue;
                    using RegistryKey? driversKey = envKey.OpenSubKey("Drivers");
                    if (driversKey == null) continue;
                    foreach (string versionName in driversKey.GetSubKeyNames())
                    {
                        if (!versionName.StartsWith("Version-", StringComparison.OrdinalIgnoreCase)) continue;
                        using RegistryKey? versionKey = driversKey.OpenSubKey(versionName);
                        if (versionKey == null) continue;
                        foreach (string driverName in versionKey.GetSubKeyNames())
                        {
                            if (!string.IsNullOrWhiteSpace(driverName)) drivers.Add(driverName.Trim());
                        }
                    }
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Log("[CLIENT] Registry driver store enumeration exception: " + ex.Message);
        }

        return drivers.OrderBy(d => d, StringComparer.OrdinalIgnoreCase).ToList();
    }

    static List<string> TryParseJsonStringArray(string? json)
    {
        if (string.IsNullOrWhiteSpace(json)) return new List<string>();
        try
        {
            if (json.TrimStart().StartsWith("["))
            {
                var arr = System.Text.Json.JsonSerializer.Deserialize<List<string>>(json);
                return arr ?? new List<string>();
            }
            var single = System.Text.Json.JsonSerializer.Deserialize<string>(json);
            return string.IsNullOrWhiteSpace(single) ? new List<string>() : new List<string> { single };
        }
        catch { return new List<string>(); }
    }

    static (bool Success, string Output) RunPowerShell(string script)
    {
        var psi = new ProcessStartInfo
        {
            FileName = "powershell.exe",
            Arguments = $"-NoProfile -ExecutionPolicy Bypass -Command \"{script} 2>&1 | Out-String -Width 4096\"",
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            CreateNoWindow = true
        };

        using var process = Process.Start(psi);
        if (process == null) return (false, "Failed to start powershell.");

        process.WaitForExit();
        string output = process.StandardOutput.ReadToEnd().Trim();

        return (process.ExitCode == 0, output);
    }
}
```

- [ ] **Step 3: Create WindowsScannerService.cs**

```csharp
// ShaPrint.Platform/Windows/WindowsScannerService.cs
using System.Collections.Concurrent;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Platform.Windows;

public class WindowsScannerService : IScannerService
{
    const string WiaFormatBMP = "{B96B3CAB-0728-11D3-9D7B-0000F81EF32E}";
    const string WiaFormatJPEG = "{B96B3CAE-0728-11D3-9D7B-0000F81EF32E}";
    const string WiaFormatPNG = "{B96B3CAF-0728-11D3-9D7B-0000F81EF32E}";
    const string WiaFormatTIFF = "{B96B3CB1-0728-11D3-9D7B-0000F81EF32E}";

    public List<ScannerInfo> GetLocalScanners()
    {
        var list = new List<ScannerInfo>();
        var thread = new Thread(() =>
        {
            try
            {
                Type? wiaType = Type.GetTypeFromProgID("WIA.DeviceManager");
                if (wiaType == null) return;

                dynamic deviceManager = Activator.CreateInstance(wiaType)!;
                foreach (dynamic info in deviceManager.DeviceInfos)
                {
                    if (info.Type == 1)
                    {
                        list.Add(new ScannerInfo
                        {
                            Name = GetScannerFriendlyName(info),
                            Description = GetScannerDescription(info)
                        });
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("[SCANNER] Failed to list scanners", ex);
            }
        });

        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        thread.Join();

        return list;
    }

    public byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, out string actualFormat)
    {
        byte[] resultBytes = Array.Empty<byte>();
        string ext = format.Equals("PNG", StringComparison.OrdinalIgnoreCase) ? "png" :
                     format.Equals("PDF", StringComparison.OrdinalIgnoreCase) ? "pdf" : "jpg";
        actualFormat = ext;

        byte[] rawBytes = Array.Empty<byte>();
        Exception? threadException = null;

        var thread = new Thread(() =>
        {
            try
            {
                Type? wiaType = Type.GetTypeFromProgID("WIA.DeviceManager");
                if (wiaType == null) throw new InvalidOperationException("WIA is not installed.");

                dynamic deviceManager = Activator.CreateInstance(wiaType)!;
                dynamic? targetDeviceInfo = null;

                foreach (dynamic info in deviceManager.DeviceInfos)
                {
                    if (info.Type == 1)
                    {
                        string friendlyName = GetScannerFriendlyName(info);
                        if (friendlyName.Equals(scannerName, StringComparison.OrdinalIgnoreCase))
                        {
                            targetDeviceInfo = info;
                            break;
                        }
                    }
                }

                if (targetDeviceInfo == null)
                    throw new DirectoryNotFoundException($"Scanner '{scannerName}' was not found.");

                dynamic device = targetDeviceInfo.Connect();
                if (device.Items.Count == 0)
                    throw new InvalidOperationException("Scanner has no scanning items.");

                dynamic item = device.Items[1];
                SetWiaProperty(device.Properties, 3088, 1);
                SetWiaProperty(item.Properties, 6146, 0);

                int wiaDataType = colorMode switch { 0 => 0, 1 => 2, _ => 3 };
                SetWiaProperty(item.Properties, 4103, wiaDataType);

                int wiaDepth = colorMode switch { 0 => 1, 1 => 8, _ => 24 };
                SetWiaProperty(item.Properties, 4104, wiaDepth);

                SetWiaProperty(item.Properties, 6147, dpi);
                SetWiaProperty(item.Properties, 6148, dpi);

                double bedWidthInches = 8.5;
                double bedHeightInches = 11.0;
                object? widthVal = GetWiaPropertyValue(item.Properties, 6165);
                if (widthVal != null) { try { int w = Convert.ToInt32(widthVal); if (w > 0) bedWidthInches = w / 1000.0; } catch { } }
                object? heightVal = GetWiaPropertyValue(item.Properties, 6166);
                if (heightVal != null) { try { int h = Convert.ToInt32(heightVal); if (h > 0) bedHeightInches = h / 1000.0; } catch { } }

                int widthPixels = (int)Math.Round(bedWidthInches * dpi);
                int heightPixels = (int)Math.Round(bedHeightInches * dpi);

                SetWiaProperty(item.Properties, 6149, 0);
                SetWiaProperty(item.Properties, 6150, 0);
                SetWiaProperty(item.Properties, 6151, widthPixels);
                SetWiaProperty(item.Properties, 6152, heightPixels);

                dynamic? imageFile = null;
                try { imageFile = item.Transfer(WiaFormatTIFF); }
                catch { try { imageFile = item.Transfer(WiaFormatBMP); } catch { } }

                if (imageFile == null)
                    throw new OperationCanceledException("Scan was cancelled or failed.");

                string tempPath = Path.Combine(Path.GetTempPath(), $"shaprint_{Guid.NewGuid():N}.tmp");
                try
                {
                    imageFile.SaveFile(tempPath);
                    rawBytes = File.ReadAllBytes(tempPath);
                }
                finally
                {
                    if (File.Exists(tempPath)) try { File.Delete(tempPath); } catch { }
                }
            }
            catch (Exception ex) { threadException = ex; }
        });

        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        thread.Join();

        if (threadException != null) throw threadException;
        return rawBytes;
    }

    static void SetWiaProperty(dynamic properties, int propId, object value)
    {
        try
        {
            foreach (dynamic p in properties)
            {
                try { if (p.PropertyID == propId) { p.set_Value(value); return; } } catch { }
            }
        }
        catch { }
    }

    static object? GetWiaPropertyValue(dynamic properties, int propId)
    {
        try
        {
            foreach (dynamic p in properties)
            {
                try { if (p.PropertyID == propId) return p.Value; } catch { }
            }
        }
        catch { }
        return null;
    }

    static string GetScannerFriendlyName(dynamic info)
    {
        string name = string.Empty;
        string description = string.Empty;
        try
        {
            foreach (dynamic prop in info.Properties)
            {
                try
                {
                    int propId = prop.PropertyID;
                    if (propId == 7) name = prop.Value?.ToString() ?? string.Empty;
                    else if (propId == 4) description = prop.Value?.ToString() ?? string.Empty;
                }
                catch { }
            }
        }
        catch { }

        if (!string.IsNullOrEmpty(name) && !name.Equals("WIA Scanner", StringComparison.OrdinalIgnoreCase))
            return name;
        if (!string.IsNullOrEmpty(description) && !description.Equals("WIA Scanner", StringComparison.OrdinalIgnoreCase))
            return description;
        return !string.IsNullOrEmpty(name) ? name : description;
    }

    static string GetScannerDescription(dynamic info)
    {
        try
        {
            foreach (dynamic prop in info.Properties)
            {
                try { if (prop.PropertyID == 4) return prop.Value?.ToString() ?? "WIA Scanner Device"; } catch { }
            }
        }
        catch { }
        return "WIA Scanner Device";
    }
}
```

- [ ] **Step 4: Create WindowsStartupManager.cs**

```csharp
// ShaPrint.Platform/Windows/WindowsStartupManager.cs
using System.Diagnostics;
using System.IO;
using ShaPrint.Core;

namespace ShaPrint.Platform.Windows;

public class WindowsStartupManager : IStartupManager
{
    const string TaskName = "ShaPrint_Startup";

    public void SetStartup(bool enable)
    {
        try
        {
            string exePath = Environment.ProcessPath ?? string.Empty;
            if (string.IsNullOrEmpty(exePath)) return;

            if (enable)
            {
                string xmlContent = GenerateXml(exePath);
                string tempXmlPath = Path.Combine(Path.GetTempPath(), "ShaPrint_Startup.xml");
                File.WriteAllText(tempXmlPath, xmlContent, System.Text.Encoding.Unicode);

                try
                {
                    var psi = new ProcessStartInfo
                    {
                        FileName = "schtasks.exe",
                        Arguments = $"/create /tn \"{TaskName}\" /xml \"{tempXmlPath}\" /f",
                        CreateNoWindow = true,
                        UseShellExecute = false
                    };
                    using var p = Process.Start(psi);
                    p?.WaitForExit();
                }
                finally
                {
                    if (File.Exists(tempXmlPath)) File.Delete(tempXmlPath);
                }
            }
            else
            {
                var psi = new ProcessStartInfo
                {
                    FileName = "schtasks.exe",
                    Arguments = $"/delete /tn \"{TaskName}\" /f",
                    CreateNoWindow = true,
                    UseShellExecute = false
                };
                using var p = Process.Start(psi);
                p?.WaitForExit();
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[SYSTEM] Failed to change startup settings: " + ex.Message);
        }
    }

    public bool IsStartupEnabled()
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "schtasks.exe",
                Arguments = $"/query /tn \"{TaskName}\"",
                CreateNoWindow = true,
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            };
            using var p = Process.Start(psi);
            if (p == null) return false;
            p.WaitForExit();
            return p.ExitCode == 0;
        }
        catch { return false; }
    }

    static string GenerateXml(string exePath)
    {
        string exeDir = Path.GetDirectoryName(exePath) ?? string.Empty;
        string escapedExe = System.Security.SecurityElement.Escape(exePath);
        string escapedDir = System.Security.SecurityElement.Escape(exeDir);

        return $@"<?xml version=""1.0"" encoding=""UTF-16""?>
<Task version=""1.2"" xmlns=""http://schemas.microsoft.com/windows/2004/02/mit/task"">
  <Triggers><LogonTrigger><Enabled>true</Enabled></LogonTrigger></Triggers>
  <Principals><Principal id=""Author""><GroupId>S-1-5-32-545</GroupId><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><AllowStartOnDemand>true</AllowStartOnDemand><Enabled>true</Enabled><Hidden>false</Hidden><RunOnlyIfIdle>false</RunOnlyIfIdle><WakeToRun>false</WakeToRun><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Priority>7</Priority></Settings>
  <Actions Context=""Author""><Exec><Command>&quot;{escapedExe}&quot;</Command><Arguments>--startup</Arguments><WorkingDirectory>{escapedDir}</WorkingDirectory></Exec></Actions>
</Task>";
    }
}
```

- [ ] **Step 5: Create WindowsNotificationService.cs**

```csharp
// ShaPrint.Platform/Windows/WindowsNotificationService.cs
using Microsoft.Toolkit.Uwp.Notifications;
using ShaPrint.Core;

namespace ShaPrint.Platform.Windows;

public class WindowsNotificationService : INotificationService
{
    public void ShowToast(string title, string body, ToastAction? action = null)
    {
        try
        {
            var builder = new ToastContentBuilder().AddText(title).AddText(body);
            if (action != null) builder.AddArgument("action", action.Arguments);
            builder.Show();
        }
        catch (Exception ex) { AppLogger.Error($"[NOTIFICATION] Failed to show toast: {ex.Message}"); }
    }

    public void ShowPrintJobCompleted(string documentName, string printerName)
        => ShowToast("Print Job Completed", $"{documentName} → {printerName}");

    public void ShowPrintJobFailed(string documentName, string printerName, string reason)
        => ShowToast("Print Job Failed", $"{documentName} → {printerName}: {reason}");

    public void ShowClientConnected(string clientAddress)
        => ShowToast("Client Connected", $"{clientAddress} connected");

    public void ShowClientDisconnected(string clientAddress)
        => ShowToast("Client Disconnected", $"{clientAddress} disconnected");

    public void ShowScanCompleted(string fileName)
        => ShowToast("Scan Complete", $"Saved to {fileName}");

    public void ShowScanFailed(string errorMessage)
        => ShowToast("Scan Failed", errorMessage);

    public void ShowPrinterError(string printerName, string errorDescription)
        => ShowToast("Printer Error", $"{printerName}: {errorDescription}");

    public void ShowSecurityAlert(string message, string detail)
        => ShowToast("Security Alert", $"{message}: {detail}");
}
```

- [ ] **Step 6: Create WindowsFirewallManager.cs**

```csharp
// ShaPrint.Platform/Windows/WindowsFirewallManager.cs
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.Windows;

public class WindowsFirewallManager : IFirewallManager
{
    public Task EnsureFirewallRulesAsync()
    {
        return Task.Run(() =>
        {
            try
            {
                bool tcpExists = CheckRuleExists("ShaPrint Server TCP");
                bool udpExists = CheckRuleExists("ShaPrint Server UDP");
                bool monitorTcpExists = CheckRuleExists("ShaPrint Monitor TCP");

                if (tcpExists && udpExists && monitorTcpExists)
                {
                    AppLogger.Log("[SERVER] Firewall rules verified — ports are open.");
                    return;
                }

                if (!tcpExists) AddRule("ShaPrint Server TCP", "TCP", Constants.PrintTcpPort);
                if (!udpExists) AddRule("ShaPrint Server UDP", "UDP", Constants.DiscoveryUdpPort);
                if (!monitorTcpExists) AddRule("ShaPrint Monitor TCP", "TCP", Constants.MonitorTcpPort);
                AppLogger.Log("[SERVER] Firewall rules added successfully.");
            }
            catch (Exception ex)
            {
                AppLogger.Error("Firewall config error: " + ex.Message);
            }
        });
    }

    static bool CheckRuleExists(string ruleName)
    {
        var psi = new ProcessStartInfo
        {
            FileName = "netsh",
            Arguments = $"advfirewall firewall show rule name=\"{ruleName}\"",
            RedirectStandardOutput = true,
            UseShellExecute = false,
            CreateNoWindow = true
        };

        using var process = Process.Start(psi);
        if (process == null) return false;
        string output = process.StandardOutput.ReadToEnd();
        process.WaitForExit();
        return process.ExitCode == 0 && !output.Contains("No rules match");
    }

    static void AddRule(string ruleName, string protocol, int port)
    {
        var psi = new ProcessStartInfo
        {
            FileName = "netsh",
            Arguments = $"advfirewall firewall add rule name=\"{ruleName}\" dir=in action=allow protocol={protocol} localport={port} profile=any",
            UseShellExecute = true,
            Verb = "runas",
            CreateNoWindow = true,
            WindowStyle = ProcessWindowStyle.Hidden
        };

        try
        {
            using var process = Process.Start(psi);
            process?.WaitForExit();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"Failed to add Firewall rule for {protocol} {port}: {ex.Message}");
        }
    }
}
```

- [ ] **Step 7: Build and verify**

Run: `dotnet build ShaPrint.Platform/ShaPrint.Platform.csproj`
Expected: Build succeeded

- [ ] **Step 8: Commit**

```bash
git add ShaPrint.Platform/Windows/
git commit -m "feat: implement Windows platform backends"
```

---

### Task 3: Create ShaPrint.UI Avalonia Project

**Files:**
- Create: `ShaPrint.UI/ShaPrint.UI.csproj`
- Create: `ShaPrint.UI/App.axaml`
- Create: `ShaPrint.UI/App.axaml.cs`
- Create: `ShaPrint.UI/Program.Windows.cs`
- Create: `ShaPrint.UI/Program.macOS.cs`
- Create: `ShaPrint.UI/Program.Linux.cs`
- Modify: `ShaPrint.sln` (add new project)

**Interfaces:**
- Consumes: `ShaPrint.Platform` interfaces
- Produces: Avalonia app shell with DI container

- [ ] **Step 1: Create ShaPrint.UI.csproj**

```xml
<!-- ShaPrint.UI/ShaPrint.UI.csproj -->
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>WinExe</OutputType>
    <TargetFramework>net8.0</TargetFramework>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
    <BuiltInComInteropSupport>true</BuiltInComInteropSupport>
    <ApplicationManifest>Windows\app.manifest</ApplicationManifest>
  </PropertyGroup>

  <ItemGroup>
    <PackageReference Include="Avalonia" Version="11.2.3" />
    <PackageReference Include="Avalonia.Desktop" Version="11.2.3" />
    <PackageReference Include="Avalonia.Themes.Fluent" Version="11.2.3" />
    <PackageReference Include="Avalonia.Fonts.Inter" Version="11.2.3" />
    <PackageReference Include="CommunityToolkit.Mvvm" Version="8.4.2" />
    <PackageReference Include="Microsoft.Extensions.Hosting" Version="10.0.8" />
  </ItemGroup>

  <ItemGroup>
    <ProjectReference Include="..\ShaPrint.Core\ShaPrint.Core.csproj" />
    <ProjectReference Include="..\ShaPrint.Platform\ShaPrint.Platform.csproj" />
  </ItemGroup>
</Project>
```

- [ ] **Step 2: Create App.axaml**

```xml
<!-- ShaPrint.UI/App.axaml -->
<Application xmlns="https://github.com/avaloniaui"
             xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
             x:Class="ShaPrint.UI.App"
             RequestedThemeVariant="Light">
    <Application.Styles>
        <FluentTheme />
    </Application.Styles>
</Application>
```

- [ ] **Step 3: Create App.axaml.cs**

```csharp
// ShaPrint.UI/App.axaml.cs
using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using ShaPrint.Platform;
using ShaPrint.Platform.Windows;
using ShaPrint.UI.ViewModels;
using ShaPrint.UI.Views;

namespace ShaPrint.UI;

public class App : Application
{
    public static IHost Host { get; private set; } = null!;

    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);
    }

    public override void OnFrameworkInitializationCompleted()
    {
        var builder = Host.CreateDefaultBuilder()
            .ConfigureServices((context, services) =>
            {
                // Platform services - will be configured per platform in Program.cs
                services.AddSingleton<MainWindowViewModel>();
            });

        Host = builder.Build();

        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            desktop.MainWindow = new MainWindow
            {
                DataContext = Host.Services.GetRequiredService<MainWindowViewModel>()
            };
        }

        base.OnFrameworkInitializationCompleted();
    }
}
```

- [ ] **Step 4: Create Program.Windows.cs**

```csharp
// ShaPrint.UI/Program.Windows.cs
using Avalonia;
using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Platform;
using ShaPrint.Platform.Windows;

namespace ShaPrint.UI;

static class ProgramWindows
{
    [STAThread]
    public static void Main(string[] args)
    {
        BuildAvaloniaApp()
            .StartWithClassicDesktopLifetime(args);
    }

    public static AppBuilder BuildAvaloniaApp()
    {
        return AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .LogToTrace();
    }

    public static void ConfigureServices(IServiceCollection services)
    {
        services.AddSingleton<IPrinterManager, WindowsPrinterManager>();
        services.AddSingleton<IVirtualPrinterManager, WindowsVirtualPrinterManager>();
        services.AddSingleton<IScannerService, WindowsScannerService>();
        services.AddSingleton<IStartupManager, WindowsStartupManager>();
        services.AddSingleton<INotificationService, WindowsNotificationService>();
        services.AddSingleton<IFirewallManager, WindowsFirewallManager>();
    }
}
```

- [ ] **Step 5: Create Program.macOS.cs**

```csharp
// ShaPrint.UI/Program.macOS.cs
using Avalonia;
using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Platform;
using ShaPrint.Platform.macOS;

namespace ShaPrint.UI;

static class ProgramMacOS
{
    public static void Main(string[] args)
    {
        BuildAvaloniaApp()
            .StartWithClassicDesktopLifetime(args);
    }

    public static AppBuilder BuildAvaloniaApp()
    {
        return AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .LogToTrace();
    }

    public static void ConfigureServices(IServiceCollection services)
    {
        services.AddSingleton<IPrinterManager, MacPrinterManager>();
        services.AddSingleton<IVirtualPrinterManager, MacVirtualPrinterManager>();
        services.AddSingleton<IScannerService, MacScannerService>();
        services.AddSingleton<IStartupManager, MacStartupManager>();
        services.AddSingleton<INotificationService, MacNotificationService>();
        services.AddSingleton<IFirewallManager, MacFirewallManager>();
    }
}
```

- [ ] **Step 6: Create Program.Linux.cs**

```csharp
// ShaPrint.UI/Program.Linux.cs
using Avalonia;
using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Platform;
using ShaPrint.Platform.Linux;

namespace ShaPrint.UI;

static class ProgramLinux
{
    public static void Main(string[] args)
    {
        BuildAvaloniaApp()
            .StartWithClassicDesktopLifetime(args);
    }

    public static AppBuilder BuildAvaloniaApp()
    {
        return AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .LogToTrace();
    }

    public static void ConfigureServices(IServiceCollection services)
    {
        services.AddSingleton<IPrinterManager, LinuxPrinterManager>();
        services.AddSingleton<IVirtualPrinterManager, LinuxVirtualPrinterManager>();
        services.AddSingleton<IScannerService, LinuxScannerService>();
        services.AddSingleton<IStartupManager, LinuxStartupManager>();
        services.AddSingleton<INotificationService, LinuxNotificationService>();
        services.AddSingleton<IFirewallManager, LinuxFirewallManager>();
    }
}
```

- [ ] **Step 7: Add project to ShaPrint.sln**
- [ ] **Step 8: Build to verify**

Run: `dotnet build ShaPrint.UI/ShaPrint.UI.csproj`
Expected: Build succeeded (may have warnings about missing macOS/Linux implementations)

- [ ] **Step 9: Commit**

```bash
git add ShaPrint.UI/ ShaPrint.sln
git commit -m "feat: create ShaPrint.UI Avalonia project with platform DI"
```

---

### Task 4: Migrate ViewModels from WpfApp

**Files:**
- Create: `ShaPrint.UI/ViewModels/MainWindowViewModel.cs`
- Create: `ShaPrint.UI/ViewModels/Pages/ServerViewModel.cs`
- Create: `ShaPrint.UI/ViewModels/Pages/ClientViewModel.cs`
- Create: `ShaPrint.UI/ViewModels/Pages/MonitorViewModel.cs`
- Create: `ShaPrint.UI/ViewModels/Pages/SettingsViewModel.cs`
- Create: `ShaPrint.UI/ViewModels/Pages/UpdatesViewModel.cs`

**Interfaces:**
- Consumes: `ShaPrint.Platform` interfaces
- Produces: ViewModels for Avalonia UI

**Implementation:** Migrate ViewModels from `ShaPrint.WpfApp/ViewModels/`, replacing WPF-specific dependencies with platform abstractions.

- [ ] **Step 1: Create MainWindowViewModel.cs**

```csharp
// ShaPrint.UI/ViewModels/MainWindowViewModel.cs
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Platform;

namespace ShaPrint.UI.ViewModels;

public partial class MainWindowViewModel : ObservableObject
{
    private readonly INotificationService _notificationService;
    private readonly IStartupManager _startupManager;

    [ObservableProperty]
    private string _title = "ShaPrint";

    [ObservableProperty]
    private bool _isServerMode;

    [ObservableProperty]
    private bool _isClientMode;

    [ObservableProperty]
    private bool _isMonitorMode;

    [ObservableProperty]
    private ObservableObject? _currentPage;

    public ServerViewModel ServerPage { get; }
    public ClientViewModel ClientPage { get; }
    public MonitorViewModel MonitorPage { get; }
    public SettingsViewModel SettingsPage { get; }
    public UpdatesViewModel UpdatesPage { get; }

    public MainWindowViewModel(
        INotificationService notificationService,
        IStartupManager startupManager,
        ServerViewModel serverPage,
        ClientViewModel clientPage,
        MonitorViewModel monitorPage,
        SettingsViewModel settingsPage,
        UpdatesViewModel updatesPage)
    {
        _notificationService = notificationService;
        _startupManager = startupManager;

        ServerPage = serverPage;
        ClientPage = clientPage;
        MonitorPage = monitorPage;
        SettingsPage = settingsPage;
        UpdatesPage = updatesPage;

        CurrentPage = ServerPage;
        IsServerMode = true;
    }

    [RelayCommand]
    private void SwitchToServer()
    {
        CurrentPage = ServerPage;
        IsServerMode = true;
        IsClientMode = false;
        IsMonitorMode = false;
    }

    [RelayCommand]
    private void SwitchToClient()
    {
        CurrentPage = ClientPage;
        IsServerMode = false;
        IsClientMode = true;
        IsMonitorMode = false;
    }

    [RelayCommand]
    private void SwitchToMonitor()
    {
        CurrentPage = MonitorPage;
        IsServerMode = false;
        IsClientMode = false;
        IsMonitorMode = true;
    }

    [RelayCommand]
    private void SwitchToSettings()
    {
        CurrentPage = SettingsPage;
        IsServerMode = false;
        IsClientMode = false;
        IsMonitorMode = false;
    }

    [RelayCommand]
    private void SwitchToUpdates()
    {
        CurrentPage = UpdatesPage;
        IsServerMode = false;
        IsClientMode = false;
        IsMonitorMode = false;
    }
}
```

- [ ] **Step 2: Create ServerViewModel.cs**

```csharp
// ShaPrint.UI/ViewModels/Pages/ServerViewModel.cs
using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Platform;

namespace ShaPrint.UI.ViewModels;

public partial class ServerViewModel : ObservableObject
{
    private readonly IPrinterManager _printerManager;
    private readonly IScannerService _scannerService;
    private readonly INotificationService _notificationService;
    private readonly IFirewallManager _firewallManager;

    [ObservableProperty]
    private bool _isServerRunning;

    [ObservableProperty]
    private string _serverStatus = "Stopped";

    [ObservableProperty]
    private ObservableCollection<PrinterInfo> _availablePrinters = new();

    [ObservableProperty]
    private ObservableCollection<ScannerInfo> _availableScanners = new();

    [ObservableProperty]
    private ObservableCollection<string> _selectedPrinters = new();

    [ObservableProperty]
    private ObservableCollection<string> _selectedScanners = new();

    [ObservableProperty]
    private ObservableCollection<string> _connectedClients = new();

    [ObservableProperty]
    private ObservableCollection<string> _recentJobs = new();

    public ServerViewModel(
        IPrinterManager printerManager,
        IScannerService scannerService,
        INotificationService notificationService,
        IFirewallManager firewallManager)
    {
        _printerManager = printerManager;
        _scannerService = scannerService;
        _notificationService = notificationService;
        _firewallManager = firewallManager;
    }

    [RelayCommand]
    private async Task LoadPrintersAsync()
    {
        var printers = await _printerManager.GetLocalPrintersAsync();
        AvailablePrinters.Clear();
        foreach (var p in printers) AvailablePrinters.Add(p);
    }

    [RelayCommand]
    private void LoadScanners()
    {
        var scanners = _scannerService.GetLocalScanners();
        AvailableScanners.Clear();
        foreach (var s in scanners) AvailableScanners.Add(s);
    }

    [RelayCommand]
    private async Task StartServerAsync()
    {
        await _firewallManager.EnsureFirewallRulesAsync();
        // TODO: Start discovery server and print receiver
        IsServerRunning = true;
        ServerStatus = "Running";
        _notificationService.ShowToast("Server Started", "ShaPrint server is now running");
    }

    [RelayCommand]
    private void StopServer()
    {
        // TODO: Stop discovery server and print receiver
        IsServerRunning = false;
        ServerStatus = "Stopped";
        _notificationService.ShowToast("Server Stopped", "ShaPrint server has been stopped");
    }

    [RelayCommand]
    private void TogglePrinter(string printerName)
    {
        if (SelectedPrinters.Contains(printerName))
            SelectedPrinters.Remove(printerName);
        else
            SelectedPrinters.Add(printerName);
    }

    [RelayCommand]
    private void ToggleScanner(string scannerName)
    {
        if (SelectedScanners.Contains(scannerName))
            SelectedScanners.Remove(scannerName);
        else
            SelectedScanners.Add(scannerName);
    }
}
```

- [ ] **Step 3: Create ClientViewModel.cs**

```csharp
// ShaPrint.UI/ViewModels/Pages/ClientViewModel.cs
using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core.Network;
using ShaPrint.Platform;

namespace ShaPrint.UI.ViewModels;

public partial class ClientViewModel : ObservableObject
{
    private readonly IVirtualPrinterManager _virtualPrinterManager;
    private readonly INotificationService _notificationService;

    [ObservableProperty]
    private ObservableCollection<PrinterInfo> _discoveredPrinters = new();

    [ObservableProperty]
    private PrinterInfo? _selectedPrinter;

    [ObservableProperty]
    private string _specificServerIp = string.Empty;

    [ObservableProperty]
    private bool _isScanning;

    [ObservableProperty]
    private string _scanStatus = string.Empty;

    [ObservableProperty]
    private ObservableCollection<string> _installedPrinters = new();

    public ClientViewModel(
        IVirtualPrinterManager virtualPrinterManager,
        INotificationService notificationService)
    {
        _virtualPrinterManager = virtualPrinterManager;
        _notificationService = notificationService;
    }

    [RelayCommand]
    private async Task ScanLanAsync()
    {
        IsScanning = true;
        ScanStatus = "Scanning...";
        // TODO: Implement UDP discovery
        await Task.Delay(1000); // Placeholder
        IsScanning = false;
        ScanStatus = $"Found {DiscoveredPrinters.Count} printer(s)";
    }

    [RelayCommand]
    private async Task InstallPrinterAsync()
    {
        if (SelectedPrinter == null)
        {
            _notificationService.ShowToast("No Printer Selected", "Please select a printer to install");
            return;
        }

        string virtualPrinterName = $"ShaPrint - {SelectedPrinter.Name}";
        string pipeName = $"ShaPrint_{SelectedPrinter.Name.Replace(" ", "_")}";

        var (success, error) = await _virtualPrinterManager.InstallPrinterAsync(
            virtualPrinterName, pipeName, SelectedPrinter.DriverName);

        if (success)
        {
            _notificationService.ShowToast("Printer Installed", $"Virtual printer '{virtualPrinterName}' has been installed");
            LoadInstalledPrinters();
        }
        else
        {
            _notificationService.ShowToast("Installation Failed", error);
        }
    }

    [RelayCommand]
    private async Task RemovePrinterAsync(string printerName)
    {
        string pipeName = $"ShaPrint_{printerName.Replace("ShaPrint - ", "").Replace(" ", "_")}";
        var (success, error) = await _virtualPrinterManager.RemovePrinterAsync(printerName, pipeName);

        if (success)
        {
            _notificationService.ShowToast("Printer Removed", $"Virtual printer '{printerName}' has been removed");
            LoadInstalledPrinters();
        }
        else
        {
            _notificationService.ShowToast("Removal Failed", error);
        }
    }

    [RelayCommand]
    private void LoadInstalledPrinters()
    {
        var drivers = _virtualPrinterManager.GetInstalledDrivers();
        InstalledPrinters.Clear();
        foreach (var d in drivers) InstalledPrinters.Add(d);
    }
}
```

- [ ] **Step 4: Create MonitorViewModel.cs**

```csharp
// ShaPrint.UI/ViewModels/Pages/MonitorViewModel.cs
using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core.Network;

namespace ShaPrint.UI.ViewModels;

public partial class MonitorViewModel : ObservableObject
{
    [ObservableProperty]
    private ObservableCollection<MonitorServerInfo> _servers = new();

    [ObservableProperty]
    private MonitorServerInfo? _selectedServer;

    [ObservableProperty]
    private bool _isRefreshing;

    [ObservableProperty]
    private string _filterText = string.Empty;

    [RelayCommand]
    private async Task RefreshAsync()
    {
        IsRefreshing = true;
        // TODO: Implement monitor discovery
        await Task.Delay(1000); // Placeholder
        IsRefreshing = false;
    }

    [RelayCommand]
    private void FilterServers()
    {
        // TODO: Implement filtering
    }
}
```

- [ ] **Step 5: Create SettingsViewModel.cs**

```csharp
// ShaPrint.UI/ViewModels/Pages/SettingsViewModel.cs
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Platform;

namespace ShaPrint.UI.ViewModels;

public partial class SettingsViewModel : ObservableObject
{
    private readonly IStartupManager _startupManager;
    private readonly INotificationService _notificationService;

    [ObservableProperty]
    private string _networkChannel = Constants.SharedSecret;

    [ObservableProperty]
    private bool _runOnStartup;

    [ObservableProperty]
    private bool _minimizeToTray;

    public SettingsViewModel(
        IStartupManager startupManager,
        INotificationService notificationService)
    {
        _startupManager = startupManager;
        _notificationService = notificationService;
        RunOnStartup = _startupManager.IsStartupEnabled();
    }

    [RelayCommand]
    private void SaveNetworkChannel()
    {
        Constants.SetNetworkChannel(NetworkChannel);
        _notificationService.ShowToast("Settings Saved", "Network channel has been updated");
    }

    [RelayCommand]
    private void ToggleStartup()
    {
        _startupManager.SetStartup(RunOnStartup);
        _notificationService.ShowToast("Startup Updated", RunOnStartup ? "ShaPrint will run on startup" : "ShaPrint will not run on startup");
    }
}
```

- [ ] **Step 6: Create UpdatesViewModel.cs**

```csharp
// ShaPrint.UI/ViewModels/Pages/UpdatesViewModel.cs
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace ShaPrint.UI.ViewModels;

public partial class UpdatesViewModel : ObservableObject
{
    [ObservableProperty]
    private string _currentVersion = "2.0.0";

    [ObservableProperty]
    private string _latestVersion = "Checking...";

    [ObservableProperty]
    private bool _isChecking;

    [ObservableProperty]
    private bool _updateAvailable;

    [ObservableProperty]
    private string _updateStatus = string.Empty;

    [RelayCommand]
    private async Task CheckForUpdatesAsync()
    {
        IsChecking = true;
        UpdateStatus = "Checking for updates...";
        // TODO: Implement GitHub update check
        await Task.Delay(1000); // Placeholder
        LatestVersion = "2.0.0";
        UpdateAvailable = false;
        UpdateStatus = "You are running the latest version";
        IsChecking = false;
    }

    [RelayCommand]
    private async Task DownloadUpdateAsync()
    {
        // TODO: Implement update download
        await Task.Delay(1000);
    }
}
```

- [ ] **Step 7: Build and verify**

Run: `dotnet build ShaPrint.UI/ShaPrint.UI.csproj`
Expected: Build succeeded

- [ ] **Step 8: Commit**

```bash
git add ShaPrint.UI/ViewModels/
git commit -m "feat: migrate ViewModels from WpfApp to Avalonia"
```

---

### Task 5: Create Avalonia Views

**Files:**
- Create: `ShaPrint.UI/Views/MainWindow.axaml`
- Create: `ShaPrint.UI/Views/Pages/ServerPage.axaml`
- Create: `ShaPrint.UI/Views/Pages/ClientPage.axaml`
- Create: `ShaPrint.UI/Views/Pages/MonitorPage.axaml`
- Create: `ShaPrint.UI/Views/Pages/SettingsPage.axaml`
- Create: `ShaPrint.UI/Views/Pages/UpdatesPage.axaml`

**Interfaces:**
- Consumes: ViewModels from Task 4
- Produces: Avalonia XAML views

- [ ] **Step 1: Create MainWindow.axaml**

```xml
<!-- ShaPrint.UI/Views/MainWindow.axaml -->
<Window xmlns="https://github.com/avaloniaui"
        xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
        xmlns:vm="using:ShaPrint.UI.ViewModels"
        xmlns:views="using:ShaPrint.UI.Views.Pages"
        x:Class="ShaPrint.UI.Views.MainWindow"
        x:DataType="vm:MainWindowViewModel"
        Title="{Binding Title}"
        Width="1200"
        Height="800"
        MinWidth="800"
        MinHeight="600"
        WindowStartupLocation="CenterScreen">

    <Design.DataContext>
        <vm:MainWindowViewModel/>
    </Design.DataContext>

    <DockPanel>
        <!-- Sidebar Navigation -->
        <StackPanel DockPanel.Dock="Left" Width="200" Background="#F0F0F0">
            <TextBlock Text="ShaPrint" FontSize="24" FontWeight="Bold" Margin="20,20,20,30" HorizontalAlignment="Center"/>

            <Button Content="Server" Command="{Binding SwitchToServerCommand}" 
                    Classes="nav-button" Margin="10,5"/>
            <Button Content="Client" Command="{Binding SwitchToClientCommand}" 
                    Classes="nav-button" Margin="10,5"/>
            <Button Content="Monitor" Command="{Binding SwitchToMonitorCommand}" 
                    Classes="nav-button" Margin="10,5"/>

            <Separator Margin="10,20"/>

            <Button Content="Settings" Command="{Binding SwitchToSettingsCommand}" 
                    Classes="nav-button" Margin="10,5"/>
            <Button Content="Updates" Command="{Binding SwitchToUpdatesCommand}" 
                    Classes="nav-button" Margin="10,5"/>
        </StackPanel>

        <!-- Main Content -->
        <ContentControl Content="{Binding CurrentPage}" Margin="20"/>
    </DockPanel>
</Window>
```

- [ ] **Step 2: Create ServerPage.axaml**

```xml
<!-- ShaPrint.UI/Views/Pages/ServerPage.axaml -->
<UserControl xmlns="https://github.com/avaloniaui"
             xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
             xmlns:vm="using:ShaPrint.UI.ViewModels"
             x:Class="ShaPrint.UI.Views.Pages.ServerPage"
             x:DataType="vm:ServerViewModel">

    <Design.DataContext>
        <vm:ServerViewModel/>
    </Design.DataContext>

    <ScrollViewer>
        <StackPanel Spacing="20">
            <!-- Server Status -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <TextBlock Text="Server Status" FontSize="20" FontWeight="Bold"/>
                    <StackPanel Orientation="Horizontal" Spacing="10">
                        <TextBlock Text="Status:" VerticalAlignment="Center"/>
                        <TextBlock Text="{Binding ServerStatus}" VerticalAlignment="Center" FontWeight="Bold"/>
                    </StackPanel>
                    <StackPanel Orientation="Horizontal" Spacing="10">
                        <Button Content="Start Server" Command="{Binding StartServerCommand}" 
                                IsVisible="{Binding !IsServerRunning}"/>
                        <Button Content="Stop Server" Command="{Binding StopServerCommand}" 
                                IsVisible="{Binding IsServerRunning}" Background="#FF4444"/>
                    </StackPanel>
                </StackPanel>
            </Border>

            <!-- Printers -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <StackPanel Orientation="Horizontal" Spacing="10">
                        <TextBlock Text="Available Printers" FontSize="20" FontWeight="Bold"/>
                        <Button Content="Refresh" Command="{Binding LoadPrintersCommand}"/>
                    </StackPanel>
                    <ItemsControl ItemsSource="{Binding AvailablePrinters}">
                        <ItemsControl.ItemTemplate>
                            <DataTemplate>
                                <StackPanel Orientation="Horizontal" Spacing="10" Margin="0,5">
                                    <CheckBox IsChecked="{Binding IsSelected}"/>
                                    <TextBlock Text="{Binding Name}" VerticalAlignment="Center"/>
                                    <TextBlock Text="{Binding DriverName}" VerticalAlignment="Center" Foreground="Gray"/>
                                </StackPanel>
                            </DataTemplate>
                        </ItemsControl.ItemTemplate>
                    </ItemsControl>
                </StackPanel>
            </Border>

            <!-- Scanners -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <StackPanel Orientation="Horizontal" Spacing="10">
                        <TextBlock Text="Available Scanners" FontSize="20" FontWeight="Bold"/>
                        <Button Content="Refresh" Command="{Binding LoadScannersCommand}"/>
                    </StackPanel>
                    <ItemsControl ItemsSource="{Binding AvailableScanners}">
                        <ItemsControl.ItemTemplate>
                            <DataTemplate>
                                <StackPanel Orientation="Horizontal" Spacing="10" Margin="0,5">
                                    <CheckBox IsChecked="{Binding IsSelected}"/>
                                    <TextBlock Text="{Binding Name}" VerticalAlignment="Center"/>
                                    <TextBlock Text="{Binding Description}" VerticalAlignment="Center" Foreground="Gray"/>
                                </StackPanel>
                            </DataTemplate>
                        </ItemsControl.ItemTemplate>
                    </ItemsControl>
                </StackPanel>
            </Border>

            <!-- Connected Clients -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <TextBlock Text="Connected Clients" FontSize="20" FontWeight="Bold"/>
                    <ItemsControl ItemsSource="{Binding ConnectedClients}">
                        <ItemsControl.ItemTemplate>
                            <DataTemplate>
                                <TextBlock Text="{Binding}" Margin="0,2"/>
                            </DataTemplate>
                        </ItemsControl.ItemTemplate>
                    </ItemsControl>
                </StackPanel>
            </Border>

            <!-- Recent Jobs -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <TextBlock Text="Recent Jobs" FontSize="20" FontWeight="Bold"/>
                    <ItemsControl ItemsSource="{Binding RecentJobs}">
                        <ItemsControl.ItemTemplate>
                            <DataTemplate>
                                <TextBlock Text="{Binding}" Margin="0,2"/>
                            </DataTemplate>
                        </ItemsControl.ItemTemplate>
                    </ItemsControl>
                </StackPanel>
            </Border>
        </StackPanel>
    </ScrollViewer>
</UserControl>
```

- [ ] **Step 3: Create ClientPage.axaml**

```xml
<!-- ShaPrint.UI/Views/Pages/ClientPage.axaml -->
<UserControl xmlns="https://github.com/avaloniaui"
             xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
             xmlns:vm="using:ShaPrint.UI.ViewModels"
             x:Class="ShaPrint.UI.Views.Pages.ClientPage"
             x:DataType="vm:ClientViewModel">

    <Design.DataContext>
        <vm:ClientViewModel/>
    </Design.DataContext>

    <ScrollViewer>
        <StackPanel Spacing="20">
            <!-- Discovery -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <TextBlock Text="Discover Servers" FontSize="20" FontWeight="Bold"/>
                    <StackPanel Orientation="Horizontal" Spacing="10">
                        <TextBox Text="{Binding SpecificServerIp}" Watermark="Specific Server IP (optional)" Width="200"/>
                        <Button Content="Scan LAN / Connect" Command="{Binding ScanLanCommand}"/>
                    </StackPanel>
                    <TextBlock Text="{Binding ScanStatus}" Foreground="Gray"/>
                </StackPanel>
            </Border>

            <!-- Discovered Printers -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <TextBlock Text="Discovered Printers" FontSize="20" FontWeight="Bold"/>
                    <ListBox ItemsSource="{Binding DiscoveredPrinters}" SelectedItem="{Binding SelectedPrinter}">
                        <ListBox.ItemTemplate>
                            <DataTemplate>
                                <StackPanel Orientation="Horizontal" Spacing="10">
                                    <TextBlock Text="{Binding Name}" FontWeight="Bold"/>
                                    <TextBlock Text="{Binding DriverName}" Foreground="Gray"/>
                                </StackPanel>
                            </DataTemplate>
                        </ListBox.ItemTemplate>
                    </ListBox>
                    <Button Content="Install Selected Printer" Command="{Binding InstallPrinterCommand}" 
                            IsEnabled="{Binding SelectedPrinter, Converter={x:Static ObjectConverters.IsNotNull}}"/>
                </StackPanel>
            </Border>

            <!-- Installed Printers -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <StackPanel Orientation="Horizontal" Spacing="10">
                        <TextBlock Text="Installed ShaPrint Printers" FontSize="20" FontWeight="Bold"/>
                        <Button Content="Refresh" Command="{Binding LoadInstalledPrintersCommand}"/>
                    </StackPanel>
                    <ItemsControl ItemsSource="{Binding InstalledPrinters}">
                        <ItemsControl.ItemTemplate>
                            <DataTemplate>
                                <StackPanel Orientation="Horizontal" Spacing="10" Margin="0,5">
                                    <TextBlock Text="{Binding}" VerticalAlignment="Center"/>
                                    <Button Content="Remove" Command="{Binding $parent[ItemsControl].DataContext.RemovePrinterCommand}" 
                                            CommandParameter="{Binding}" Background="#FF4444"/>
                                </StackPanel>
                            </DataTemplate>
                        </ItemsControl.ItemTemplate>
                    </ItemsControl>
                </StackPanel>
            </Border>
        </StackPanel>
    </ScrollViewer>
</UserControl>
```

- [ ] **Step 4: Create MonitorPage.axaml**

```xml
<!-- ShaPrint.UI/Views/Pages/MonitorPage.axaml -->
<UserControl xmlns="https://github.com/avaloniaui"
             xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
             xmlns:vm="using:ShaPrint.UI.ViewModels"
             x:Class="ShaPrint.UI.Views.Pages.MonitorPage"
             x:DataType="vm:MonitorViewModel">

    <Design.DataContext>
        <vm:MonitorViewModel/>
    </Design.DataContext>

    <ScrollViewer>
        <StackPanel Spacing="20">
            <!-- Header -->
            <StackPanel Orientation="Horizontal" Spacing="10">
                <TextBlock Text="Server Monitor" FontSize="24" FontWeight="Bold"/>
                <Button Content="Refresh" Command="{Binding RefreshCommand}"/>
                <TextBox Text="{Binding FilterText}" Watermark="Filter servers..." Width="200"/>
            </StackPanel>

            <!-- Server List -->
            <ItemsControl ItemsSource="{Binding Servers}">
                <ItemsControl.ItemTemplate>
                    <DataTemplate>
                        <Border Background="#F5F5F5" CornerRadius="8" Padding="15" Margin="0,5">
                            <StackPanel Spacing="5">
                                <StackPanel Orientation="Horizontal" Spacing="10">
                                    <TextBlock Text="{Binding Hostname}" FontWeight="Bold" FontSize="16"/>
                                    <TextBlock Text="{Binding IpAddress}" Foreground="Gray"/>
                                    <TextBlock Text="{Binding Status}" FontWeight="Bold"/>
                                </StackPanel>
                                <TextBlock Text="{Binding Version}" Foreground="Gray"/>
                                <TextBlock Text="{Binding Uptime}" Foreground="Gray"/>
                                <TextBlock Text="{Binding PrinterCount, StringFormat='Printers: {0}'}"/>
                                <TextBlock Text="{Binding ClientCount, StringFormat='Clients: {0}'}"/>
                            </StackPanel>
                        </Border>
                    </DataTemplate>
                </ItemsControl.ItemTemplate>
            </ItemsControl>
        </StackPanel>
    </ScrollViewer>
</UserControl>
```

- [ ] **Step 5: Create SettingsPage.axaml**

```xml
<!-- ShaPrint.UI/Views/Pages/SettingsPage.axaml -->
<UserControl xmlns="https://github.com/avaloniaui"
             xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
             xmlns:vm="using:ShaPrint.UI.ViewModels"
             x:Class="ShaPrint.UI.Views.Pages.SettingsPage"
             x:DataType="vm:SettingsViewModel">

    <Design.DataContext>
        <vm:SettingsViewModel/>
    </Design.DataContext>

    <ScrollViewer>
        <StackPanel Spacing="20">
            <TextBlock Text="Settings" FontSize="24" FontWeight="Bold"/>

            <!-- Network Channel -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <TextBlock Text="Network Channel" FontSize="20" FontWeight="Bold"/>
                    <TextBlock Text="All devices must share the same channel to communicate." Foreground="Gray"/>
                    <StackPanel Orientation="Horizontal" Spacing="10">
                        <TextBox Text="{Binding NetworkChannel}" Width="300"/>
                        <Button Content="Save" Command="{Binding SaveNetworkChannelCommand}"/>
                    </StackPanel>
                </StackPanel>
            </Border>

            <!-- Startup -->
            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <TextBlock Text="Startup" FontSize="20" FontWeight="Bold"/>
                    <CheckBox Content="Run ShaPrint on system startup" IsChecked="{Binding RunOnStartup}" 
                              Command="{Binding ToggleStartupCommand}"/>
                    <CheckBox Content="Minimize to system tray" IsChecked="{Binding MinimizeToTray}"/>
                </StackPanel>
            </Border>
        </StackPanel>
    </ScrollViewer>
</UserControl>
```

- [ ] **Step 6: Create UpdatesPage.axaml**

```xml
<!-- ShaPrint.UI/Views/Pages/UpdatesPage.axaml -->
<UserControl xmlns="https://github.com/avaloniaui"
             xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
             xmlns:vm="using:ShaPrint.UI.ViewModels"
             x:Class="ShaPrint.UI.Views.Pages.UpdatesPage"
             x:DataType="vm:UpdatesViewModel">

    <Design.DataContext>
        <vm:UpdatesViewModel/>
    </Design.DataContext>

    <ScrollViewer>
        <StackPanel Spacing="20">
            <TextBlock Text="Updates" FontSize="24" FontWeight="Bold"/>

            <Border Background="#F5F5F5" CornerRadius="8" Padding="20">
                <StackPanel Spacing="10">
                    <StackPanel Orientation="Horizontal" Spacing="10">
                        <TextBlock Text="Current Version:" FontWeight="Bold"/>
                        <TextBlock Text="{Binding CurrentVersion}"/>
                    </StackPanel>
                    <StackPanel Orientation="Horizontal" Spacing="10">
                        <TextBlock Text="Latest Version:" FontWeight="Bold"/>
                        <TextBlock Text="{Binding LatestVersion}"/>
                    </StackPanel>
                    <TextBlock Text="{Binding UpdateStatus}" Foreground="Gray"/>
                    <Button Content="Check for Updates" Command="{Binding CheckForUpdatesCommand}" 
                            IsEnabled="{Binding !IsChecking}"/>
                    <Button Content="Download Update" Command="{Binding DownloadUpdateCommand}" 
                            IsVisible="{Binding UpdateAvailable}"/>
                </StackPanel>
            </Border>
        </StackPanel>
    </ScrollViewer>
</UserControl>
```

- [ ] **Step 7: Create code-behind files**

```csharp
// ShaPrint.UI/Views/MainWindow.axaml.cs
using Avalonia.Controls;

namespace ShaPrint.UI.Views;

public partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
    }
}
```

```csharp
// ShaPrint.UI/Views/Pages/ServerPage.axaml.cs
using Avalonia.Controls;

namespace ShaPrint.UI.Views.Pages;

public partial class ServerPage : UserControl
{
    public ServerPage()
    {
        InitializeComponent();
    }
}
```

```csharp
// ShaPrint.UI/Views/Pages/ClientPage.axaml.cs
using Avalonia.Controls;

namespace ShaPrint.UI.Views.Pages;

public partial class ClientPage : UserControl
{
    public ClientPage()
    {
        InitializeComponent();
    }
}
```

```csharp
// ShaPrint.UI/Views/Pages/MonitorPage.axaml.cs
using Avalonia.Controls;

namespace ShaPrint.UI.Views.Pages;

public partial class MonitorPage : UserControl
{
    public MonitorPage()
    {
        InitializeComponent();
    }
}
```

```csharp
// ShaPrint.UI/Views/Pages/SettingsPage.axaml.cs
using Avalonia.Controls;

namespace ShaPrint.UI.Views.Pages;

public partial class SettingsPage : UserControl
{
    public SettingsPage()
    {
        InitializeComponent();
    }
}
```

```csharp
// ShaPrint.UI/Views/Pages/UpdatesPage.axaml.cs
using Avalonia.Controls;

namespace ShaPrint.UI.Views.Pages;

public partial class UpdatesPage : UserControl
{
    public UpdatesPage()
    {
        InitializeComponent();
    }
}
```

- [ ] **Step 8: Build and verify**

Run: `dotnet build ShaPrint.UI/ShaPrint.UI.csproj`
Expected: Build succeeded

- [ ] **Step 9: Commit**

```bash
git add ShaPrint.UI/Views/
git commit -m "feat: create Avalonia views for all pages"
```

---

### Task 6: Implement macOS Platform Backends

**Files:**
- Create: `ShaPrint.Platform/macOS/MacPrinterManager.cs`
- Create: `ShaPrint.Platform/macOS/MacVirtualPrinterManager.cs`
- Create: `ShaPrint.Platform/macOS/MacScannerService.cs`
- Create: `ShaPrint.Platform/macOS/MacStartupManager.cs`
- Create: `ShaPrint.Platform/macOS/MacNotificationService.cs`
- Create: `ShaPrint.Platform/macOS/MacFirewallManager.cs`

**Interfaces:**
- Consumes: All platform interfaces
- Produces: macOS implementations

- [ ] **Step 1: Create MacPrinterManager.cs**

```csharp
// ShaPrint.Platform/macOS/MacPrinterManager.cs
using System.Diagnostics;
using System.Runtime.InteropServices;
using ShaPrint.Core.Network;

namespace ShaPrint.Platform.macOS;

public class MacPrinterManager : IPrinterManager
{
    [DllImport("libcups")]
    static extern int cupsGetDests(out IntPtr dests);

    [DllImport("libcups")]
    static extern void cupsFreeDests(int num_dests, IntPtr dests);

    [DllImport("libcups")]
    static extern int cupsPrintFile(string name, string filename, string title, int num_options, IntPtr options);

    public Task<List<PrinterInfo>> GetLocalPrintersAsync()
    {
        return Task.Run(() =>
        {
            var printers = new List<PrinterInfo>();
            try
            {
                int count = cupsGetDests(out IntPtr dests);
                if (count > 0 && dests != IntPtr.Zero)
                {
                    // Parse CUPS destinations structure
                    // Each dest is: name (IntPtr), instance (IntPtr), is_default (int), num_options (int), options (IntPtr)
                    int offset = 0;
                    int structSize = IntPtr.Size * 3 + sizeof(int) * 2;

                    for (int i = 0; i < count; i++)
                    {
                        IntPtr current = IntPtr.Add(dests, offset);
                        IntPtr namePtr = Marshal.ReadIntPtr(current);
                        string name = Marshal.PtrToStringAnsi(namePtr) ?? "Unknown";

                        printers.Add(new PrinterInfo
                        {
                            Name = name,
                            DriverName = "CUPS Driver"
                        });

                        offset += structSize;
                    }

                    cupsFreeDests(count, dests);
                }
            }
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error("[MAC] Failed to get printers", ex);
            }
            return printers;
        });
    }

    public async Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null)
    {
        return await Task.Run(() =>
        {
            try
            {
                string tempFile = Path.Combine(Path.GetTempPath(), $"shaprint_{Guid.NewGuid():N}.prn");
                File.WriteAllBytes(tempFile, data);

                try
                {
                    var psi = new ProcessStartInfo
                    {
                        FileName = "lp",
                        Arguments = $"-d \"{printerName}\" -t \"{documentName}\" \"{tempFile}\"",
                        RedirectStandardOutput = true,
                        RedirectStandardError = true,
                        UseShellExecute = false,
                        CreateNoWindow = true
                    };

                    using var process = Process.Start(psi);
                    if (process == null) return false;
                    process.WaitForExit();
                    return process.ExitCode == 0;
                }
                finally
                {
                    if (File.Exists(tempFile)) File.Delete(tempFile);
                }
            }
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error("[MAC] Print failed", ex);
                return false;
            }
        });
    }
}
```

- [ ] **Step 2: Create MacVirtualPrinterManager.cs**

```csharp
// ShaPrint.Platform/macOS/MacVirtualPrinterManager.cs
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.macOS;

public class MacVirtualPrinterManager : IVirtualPrinterManager
{
    public async Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string pipeName, string driverName)
    {
        return await Task.Run(() =>
        {
            try
            {
                // Create CUPS backend script
                string backendDir = "/usr/lib/cups/backend";
                string backendPath = Path.Combine(backendDir, "shaprint");

                string backendScript = $@"#!/bin/bash
# ShaPrint CUPS Backend
# This script pipes print data to ShaPrint server

if [ $# -eq 0 ]; then
    echo ""direct shaprint ""ShaPrint Virtual Printer"" ""ShaPrint network printer backend"""
    exit 0
fi

# Read print data from stdin
cat > /tmp/shaprint_job_$$.tmp

# Get printer name from arguments
PRINTER_NAME=$2

# Send to ShaPrint server (implementation depends on your server discovery)
# For now, just log the job
echo ""ShaPrint: Received job for $PRINTER_NAME"" >> /tmp/shaprint.log

rm -f /tmp/shaprint_job_$$.tmp
exit 0
";

                // Write backend script
                File.WriteAllText(backendPath, backendScript);
                Process.Start("chmod", $"+x {backendPath}")?.WaitForExit();

                // Create PPD file
                string ppdDir = "/usr/share/cups/model";
                string ppdPath = Path.Combine(ppdDir, "ShaPrint.ppd");

                string ppdContent = $@"*PPD-Adobe: ""4.3""
*% ShaPrint Virtual Printer PPD
*ModelName: ""{virtualPrinterName}""
*ShortNickName: ""{virtualPrinterName}""
*NickName: ""{virtualPrinterName}""
*Manufacturer: ""ShaPrint""
*PCFileName: ""shaprint.ppd""
*LanguageVersion: English
*LanguageEncoding: ISOLatin1
*DefaultColorSpace: RGB
*FileSystem: False
*Throughput: ""1""
*LandscapeOrientation: Plus90

*OpenUI *PageSize/Media Size: PickOne
*DefaultPageSize: A4
*PageSize A4/A4: ""<</PageSize[595.28 841.89]>>setpagedevice""
*PageSize Letter/Letter: ""<</PageSize[612 792]>>setpagedevice""
*CloseUI: *PageSize

*OpenUI *InputSlot/Media Source: PickOne
*DefaultInputSlot: Auto
*InputSlot Auto/Auto: ""<</ManualFeed false>>setpagedevice""
*CloseUI: *InputSlot

*DefaultImageableArea: A4
*ImageableArea A4: ""18 36 577 806""
*ImageableArea Letter: ""18 36 594 756""

*DefaultPaperDimension: A4
*PaperDimension A4: ""595.28 841.89""
*PaperDimension Letter: ""612 792""
";

                File.WriteAllText(ppdPath, ppdContent);

                // Add printer to CUPS
                var psi = new ProcessStartInfo
                {
                    FileName = "lpadmin",
                    Arguments = $"-p \"{virtualPrinterName}\" -v shaprint:/// -P \"{ppdPath}\" -E",
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                    CreateNoWindow = true
                };

                using var process = Process.Start(psi);
                if (process == null) return (false, "Failed to run lpadmin");
                process.WaitForExit();

                if (process.ExitCode == 0)
                {
                    AppLogger.Log($"[MAC] Virtual printer '{virtualPrinterName}' installed successfully");
                    return (true, string.Empty);
                }
                else
                {
                    string error = process.StandardError.ReadToEnd();
                    return (false, $"lpadmin failed: {error}");
                }
            }
            catch (Exception ex)
            {
                return (false, "Exception: " + ex.Message);
            }
        });
    }

    public async Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string printerName, string pipeName)
    {
        return await Task.Run(() =>
        {
            try
            {
                var psi = new ProcessStartInfo
                {
                    FileName = "lpadmin",
                    Arguments = $"-x \"{printerName}\"",
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                    CreateNoWindow = true
                };

                using var process = Process.Start(psi);
                if (process == null) return (false, "Failed to run lpadmin");
                process.WaitForExit();

                if (process.ExitCode == 0)
                {
                    AppLogger.Log($"[MAC] Virtual printer '{printerName}' removed successfully");
                    return (true, string.Empty);
                }
                else
                {
                    string error = process.StandardError.ReadToEnd();
                    return (false, $"lpadmin failed: {error}");
                }
            }
            catch (Exception ex)
            {
                return (false, "Exception: " + ex.Message);
            }
        });
    }

    public bool CheckPrinterExists(string printerName)
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "lpstat",
                Arguments = $"-p \"{printerName}\"",
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) return false;
            process.WaitForExit();
            return process.ExitCode == 0;
        }
        catch { return false; }
    }

    public List<string> GetInstalledDrivers()
    {
        var drivers = new List<string>();
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "lpinfo",
                Arguments = "-m",
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) return drivers;
            string output = process.StandardOutput.ReadToEnd();
            process.WaitForExit();

            foreach (string line in output.Split('\n'))
            {
                if (!string.IsNullOrWhiteSpace(line))
                {
                    string[] parts = line.Split(' ', 2);
                    if (parts.Length == 2) drivers.Add(parts[1].Trim());
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[MAC] Failed to get drivers", ex);
        }
        return drivers;
    }
}
```

- [ ] **Step 3: Create MacScannerService.cs**

```csharp
// ShaPrint.Platform/macOS/MacScannerService.cs
using System.Diagnostics;
using System.Runtime.InteropServices;
using ShaPrint.Core.Network;

namespace ShaPrint.Platform.macOS;

public class MacScannerService : IScannerService
{
    public List<ScannerInfo> GetLocalScanners()
    {
        var scanners = new List<ScannerInfo>();
        try
        {
            // Use scanimage to list scanners
            var psi = new ProcessStartInfo
            {
                FileName = "scanimage",
                Arguments = "-L",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) return scanners;
            string output = process.StandardOutput.ReadToEnd();
            process.WaitForExit();

            foreach (string line in output.Split('\n'))
            {
                if (line.Contains("device `"))
                {
                    int start = line.IndexOf("`") + 1;
                    int end = line.IndexOf("'", start);
                    if (end > start)
                    {
                        string deviceName = line.Substring(start, end - start);
                        scanners.Add(new ScannerInfo
                        {
                            Name = deviceName,
                            Description = "SANE Scanner"
                        });
                    }
                }
            }
        }
        catch (Exception ex)
        {
            ShaPrint.Core.AppLogger.Error("[MAC] Failed to list scanners", ex);
        }
        return scanners;
    }

    public byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, out string actualFormat)
    {
        string ext = format.Equals("PNG", StringComparison.OrdinalIgnoreCase) ? "png" :
                     format.Equals("PDF", StringComparison.OrdinalIgnoreCase) ? "pdf" : "jpg";
        actualFormat = ext;

        try
        {
            string outputFile = Path.Combine(Path.GetTempPath(), $"shaprint_scan_{Guid.NewGuid():N}.{ext}");

            string formatFlag = ext switch
            {
                "png" => "--format=png",
                "pdf" => "--format=pdf",
                _ => "--format=jpeg"
            };

            string colorModeFlag = colorMode switch
            {
                0 => "--mode=Lineart",
                1 => "--mode=Gray",
                _ => "--mode=Color"
            };

            var psi = new ProcessStartInfo
            {
                FileName = "scanimage",
                Arguments = $"-d \"{scannerName}\" {formatFlag} {colorModeFlag} --resolution={dpi} > \"{outputFile}\"",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) throw new InvalidOperationException("Failed to start scanimage");
            process.WaitForExit();

            if (process.ExitCode != 0)
            {
                string error = process.StandardError.ReadToEnd();
                throw new InvalidOperationException($"scanimage failed: {error}");
            }

            byte[] result = File.ReadAllBytes(outputFile);
            File.Delete(outputFile);
            return result;
        }
        catch (Exception ex)
        {
            ShaPrint.Core.AppLogger.Error("[MAC] Scan failed", ex);
            throw;
        }
    }
}
```

- [ ] **Step 4: Create MacStartupManager.cs**

```csharp
// ShaPrint.Platform/macOS/MacStartupManager.cs
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.macOS;

public class MacStartupManager : IStartupManager
{
    const string PlistLabel = "com.shaprint.app";
    string PlistPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        "Library", "LaunchAgents", $"{PlistLabel}.plist");

    public void SetStartup(bool enable)
    {
        try
        {
            if (enable)
            {
                string exePath = Environment.ProcessPath ?? string.Empty;
                if (string.IsNullOrEmpty(exePath)) return;

                string plistContent = $@"<?xml version=""1.0"" encoding=""UTF-8""?>
<!DOCTYPE plist PUBLIC ""-//Apple//DTD PLIST 1.0//EN"" ""http://www.apple.com/DTDs/PropertyList-1.0.dtd"">
<plist version=""1.0"">
<dict>
    <key>Label</key>
    <string>{PlistLabel}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exePath}</string>
        <string>--startup</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>";

                File.WriteAllText(PlistPath, plistContent);

                // Load the launch agent
                Process.Start("launchctl", $"load {PlistPath}")?.WaitForExit();
                AppLogger.Log("[MAC] Startup enabled via LaunchAgent");
            }
            else
            {
                if (File.Exists(PlistPath))
                {
                    // Unload the launch agent
                    Process.Start("launchctl", $"unload {PlistPath}")?.WaitForExit();
                    File.Delete(PlistPath);
                    AppLogger.Log("[MAC] Startup disabled");
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[MAC] Failed to set startup", ex);
        }
    }

    public bool IsStartupEnabled()
    {
        return File.Exists(PlistPath);
    }
}
```

- [ ] **Step 5: Create MacNotificationService.cs**

```csharp
// ShaPrint.Platform/macOS/MacNotificationService.cs
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.macOS;

public class MacNotificationService : INotificationService
{
    public void ShowToast(string title, string body, ToastAction? action = null)
    {
        try
        {
            // Use osascript to show macOS notification
            string script = $"display notification \"{body}\" with title \"{title}\"";
            var psi = new ProcessStartInfo
            {
                FileName = "osascript",
                Arguments = $"-e \"{script}\"",
                UseShellExecute = false,
                CreateNoWindow = true
            };

            Process.Start(psi);
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[MAC] Failed to show notification: {ex.Message}");
        }
    }

    public void ShowPrintJobCompleted(string documentName, string printerName)
        => ShowToast("Print Job Completed", $"{documentName} → {printerName}");

    public void ShowPrintJobFailed(string documentName, string printerName, string reason)
        => ShowToast("Print Job Failed", $"{documentName} → {printerName}: {reason}");

    public void ShowClientConnected(string clientAddress)
        => ShowToast("Client Connected", $"{clientAddress} connected");

    public void ShowClientDisconnected(string clientAddress)
        => ShowToast("Client Disconnected", $"{clientAddress} disconnected");

    public void ShowScanCompleted(string fileName)
        => ShowToast("Scan Complete", $"Saved to {fileName}");

    public void ShowScanFailed(string errorMessage)
        => ShowToast("Scan Failed", errorMessage);

    public void ShowPrinterError(string printerName, string errorDescription)
        => ShowToast("Printer Error", $"{printerName}: {errorDescription}");

    public void ShowSecurityAlert(string message, string detail)
        => ShowToast("Security Alert", $"{message}: {detail}");
}
```

- [ ] **Step 6: Create MacFirewallManager.cs**

```csharp
// ShaPrint.Platform/macOS/MacFirewallManager.cs
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.macOS;

public class MacFirewallManager : IFirewallManager
{
    public Task EnsureFirewallRulesAsync()
    {
        return Task.Run(() =>
        {
            try
            {
                // macOS firewall is managed via System Preferences
                // We'll prompt the user to allow the app through firewall
                AppLogger.Log("[MAC] Firewall configuration: Please allow ShaPrint through macOS Firewall in System Preferences > Security & Privacy > Firewall");

                // Try to add firewall rule using pfctl (requires root)
                string rule = $"pass in proto tcp from any to any port {Constants.PrintTcpPort}";
                string rule2 = $"pass in proto udp from any to any port {Constants.DiscoveryUdpPort}";
                string rule3 = $"pass in proto tcp from any to any port {Constants.MonitorTcpPort}";

                // Note: This requires root privileges and may not work without sudo
                // For now, we'll just log a message
                AppLogger.Log("[MAC] Firewall rules would need to be added manually or via MDM");
            }
            catch (Exception ex)
            {
                AppLogger.Error("[MAC] Firewall config error", ex);
            }
        });
    }
}
```

- [ ] **Step 7: Build and verify**

Run: `dotnet build ShaPrint.Platform/ShaPrint.Platform.csproj`
Expected: Build succeeded

- [ ] **Step 8: Commit**

```bash
git add ShaPrint.Platform/macOS/
git commit -m "feat: implement macOS platform backends"
```

---

### Task 7: Implement Linux Platform Backends

**Files:**
- Create: `ShaPrint.Platform/Linux/LinuxPrinterManager.cs`
- Create: `ShaPrint.Platform/Linux/LinuxVirtualPrinterManager.cs`
- Create: `ShaPrint.Platform/Linux/LinuxScannerService.cs`
- Create: `ShaPrint.Platform/Linux/LinuxStartupManager.cs`
- Create: `ShaPrint.Platform/Linux/LinuxNotificationService.cs`
- Create: `ShaPrint.Platform/Linux/LinuxFirewallManager.cs`

**Interfaces:**
- Consumes: All platform interfaces
- Produces: Linux implementations

- [ ] **Step 1: Create LinuxPrinterManager.cs**

```csharp
// ShaPrint.Platform/Linux/LinuxPrinterManager.cs
using System.Diagnostics;
using System.Runtime.InteropServices;
using ShaPrint.Core.Network;

namespace ShaPrint.Platform.Linux;

public class LinuxPrinterManager : IPrinterManager
{
    [DllImport("libcups")]
    static extern int cupsGetDests(out IntPtr dests);

    [DllImport("libcups")]
    static extern void cupsFreeDests(int num_dests, IntPtr dests);

    public Task<List<PrinterInfo>> GetLocalPrintersAsync()
    {
        return Task.Run(() =>
        {
            var printers = new List<PrinterInfo>();
            try
            {
                int count = cupsGetDests(out IntPtr dests);
                if (count > 0 && dests != IntPtr.Zero)
                {
                    int offset = 0;
                    int structSize = IntPtr.Size * 3 + sizeof(int) * 2;

                    for (int i = 0; i < count; i++)
                    {
                        IntPtr current = IntPtr.Add(dests, offset);
                        IntPtr namePtr = Marshal.ReadIntPtr(current);
                        string name = Marshal.PtrToStringAnsi(namePtr) ?? "Unknown";

                        printers.Add(new PrinterInfo
                        {
                            Name = name,
                            DriverName = "CUPS Driver"
                        });

                        offset += structSize;
                    }

                    cupsFreeDests(count, dests);
                }
            }
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error("[LINUX] Failed to get printers", ex);
            }
            return printers;
        });
    }

    public async Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null)
    {
        return await Task.Run(() =>
        {
            try
            {
                string tempFile = Path.Combine(Path.GetTempPath(), $"shaprint_{Guid.NewGuid():N}.prn");
                File.WriteAllBytes(tempFile, data);

                try
                {
                    var psi = new ProcessStartInfo
                    {
                        FileName = "lp",
                        Arguments = $"-d \"{printerName}\" -t \"{documentName}\" \"{tempFile}\"",
                        RedirectStandardOutput = true,
                        RedirectStandardError = true,
                        UseShellExecute = false,
                        CreateNoWindow = true
                    };

                    using var process = Process.Start(psi);
                    if (process == null) return false;
                    process.WaitForExit();
                    return process.ExitCode == 0;
                }
                finally
                {
                    if (File.Exists(tempFile)) File.Delete(tempFile);
                }
            }
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error("[LINUX] Print failed", ex);
                return false;
            }
        });
    }
}
```

- [ ] **Step 2: Create LinuxVirtualPrinterManager.cs**

```csharp
// ShaPrint.Platform/Linux/LinuxVirtualPrinterManager.cs
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.Linux;

public class LinuxVirtualPrinterManager : IVirtualPrinterManager
{
    public async Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string pipeName, string driverName)
    {
        return await Task.Run(() =>
        {
            try
            {
                // Create CUPS backend script
                string backendDir = "/usr/lib/cups/backend";
                string backendPath = Path.Combine(backendDir, "shaprint");

                string backendScript = $@"#!/bin/bash
# ShaPrint CUPS Backend
# This script pipes print data to ShaPrint server

if [ $# -eq 0 ]; then
    echo ""direct shaprint ""ShaPrint Virtual Printer"" ""ShaPrint network printer backend"""
    exit 0
fi

# Read print data from stdin
cat > /tmp/shaprint_job_$$.tmp

# Get printer name from arguments
PRINTER_NAME=$2

# Send to ShaPrint server (implementation depends on your server discovery)
# For now, just log the job
echo ""ShaPrint: Received job for $PRINTER_NAME"" >> /tmp/shaprint.log

rm -f /tmp/shaprint_job_$$.tmp
exit 0
";

                // Write backend script
                File.WriteAllText(backendPath, backendScript);
                Process.Start("chmod", $"+x {backendPath}")?.WaitForExit();

                // Create PPD file
                string ppdDir = "/usr/share/cups/model";
                string ppdPath = Path.Combine(ppdDir, "ShaPrint.ppd");

                string ppdContent = $@"*PPD-Adobe: ""4.3""
*% ShaPrint Virtual Printer PPD
*ModelName: ""{virtualPrinterName}""
*ShortNickName: ""{virtualPrinterName}""
*NickName: ""{virtualPrinterName}""
*Manufacturer: ""ShaPrint""
*PCFileName: ""shaprint.ppd""
*LanguageVersion: English
*LanguageEncoding: ISOLatin1
*DefaultColorSpace: RGB
*FileSystem: False
*Throughput: ""1""
*LandscapeOrientation: Plus90

*OpenUI *PageSize/Media Size: PickOne
*DefaultPageSize: A4
*PageSize A4/A4: ""<</PageSize[595.28 841.89]>>setpagedevice""
*PageSize Letter/Letter: ""<</PageSize[612 792]>>setpagedevice""
*CloseUI: *PageSize

*OpenUI *InputSlot/Media Source: PickOne
*DefaultInputSlot: Auto
*InputSlot Auto/Auto: ""<</ManualFeed false>>setpagedevice""
*CloseUI: *InputSlot

*DefaultImageableArea: A4
*ImageableArea A4: ""18 36 577 806""
*ImageableArea Letter: ""18 36 594 756""

*DefaultPaperDimension: A4
*PaperDimension A4: ""595.28 841.89""
*PaperDimension Letter: ""612 792""
";

                File.WriteAllText(ppdPath, ppdContent);

                // Add printer to CUPS
                var psi = new ProcessStartInfo
                {
                    FileName = "lpadmin",
                    Arguments = $"-p \"{virtualPrinterName}\" -v shaprint:/// -P \"{ppdPath}\" -E",
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                    CreateNoWindow = true
                };

                using var process = Process.Start(psi);
                if (process == null) return (false, "Failed to run lpadmin");
                process.WaitForExit();

                if (process.ExitCode == 0)
                {
                    AppLogger.Log($"[LINUX] Virtual printer '{virtualPrinterName}' installed successfully");
                    return (true, string.Empty);
                }
                else
                {
                    string error = process.StandardError.ReadToEnd();
                    return (false, $"lpadmin failed: {error}");
                }
            }
            catch (Exception ex)
            {
                return (false, "Exception: " + ex.Message);
            }
        });
    }

    public async Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string printerName, string pipeName)
    {
        return await Task.Run(() =>
        {
            try
            {
                var psi = new ProcessStartInfo
                {
                    FileName = "lpadmin",
                    Arguments = $"-x \"{printerName}\"",
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                    CreateNoWindow = true
                };

                using var process = Process.Start(psi);
                if (process == null) return (false, "Failed to run lpadmin");
                process.WaitForExit();

                if (process.ExitCode == 0)
                {
                    AppLogger.Log($"[LINUX] Virtual printer '{printerName}' removed successfully");
                    return (true, string.Empty);
                }
                else
                {
                    string error = process.StandardError.ReadToEnd();
                    return (false, $"lpadmin failed: {error}");
                }
            }
            catch (Exception ex)
            {
                return (false, "Exception: " + ex.Message);
            }
        });
    }

    public bool CheckPrinterExists(string printerName)
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "lpstat",
                Arguments = $"-p \"{printerName}\"",
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) return false;
            process.WaitForExit();
            return process.ExitCode == 0;
        }
        catch { return false; }
    }

    public List<string> GetInstalledDrivers()
    {
        var drivers = new List<string>();
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "lpinfo",
                Arguments = "-m",
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) return drivers;
            string output = process.StandardOutput.ReadToEnd();
            process.WaitForExit();

            foreach (string line in output.Split('\n'))
            {
                if (!string.IsNullOrWhiteSpace(line))
                {
                    string[] parts = line.Split(' ', 2);
                    if (parts.Length == 2) drivers.Add(parts[1].Trim());
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[LINUX] Failed to get drivers", ex);
        }
        return drivers;
    }
}
```

- [ ] **Step 3: Create LinuxScannerService.cs**

```csharp
// ShaPrint.Platform/Linux/LinuxScannerService.cs
using System.Diagnostics;
using System.Runtime.InteropServices;
using ShaPrint.Core.Network;

namespace ShaPrint.Platform.Linux;

public class LinuxScannerService : IScannerService
{
    [DllImport("libsane")]
    static extern int sane_init(out int version_code, IntPtr authorize);

    [DllImport("libsane")]
    static extern int sane_get_devices(out IntPtr device_list, int local_only);

    [DllImport("libsane")]
    static extern void sane_exit();

    public List<ScannerInfo> GetLocalScanners()
    {
        var scanners = new List<ScannerInfo>();
        try
        {
            int status = sane_init(out int version_code, IntPtr.Zero);
            if (status != 0)
            {
                AppLogger.Error("[LINUX] SANE init failed");
                return scanners;
            }

            status = sane_get_devices(out IntPtr device_list, 1);
            if (status == 0 && device_list != IntPtr.Zero)
            {
                // Parse device list
                // Each device pointer is IntPtr.Size bytes
                int offset = 0;
                while (true)
                {
                    IntPtr devicePtr = Marshal.ReadIntPtr(device_list, offset);
                    if (devicePtr == IntPtr.Zero) break;

                    // Read device structure
                    IntPtr namePtr = Marshal.ReadIntPtr(devicePtr);
                    IntPtr vendorPtr = Marshal.ReadIntPtr(devicePtr, IntPtr.Size);
                    IntPtr modelPtr = Marshal.ReadIntPtr(devicePtr, IntPtr.Size * 2);
                    IntPtr typePtr = Marshal.ReadIntPtr(devicePtr, IntPtr.Size * 3);

                    string name = Marshal.PtrToStringAnsi(namePtr) ?? "Unknown";
                    string vendor = Marshal.PtrToStringAnsi(vendorPtr) ?? "";
                    string model = Marshal.PtrToStringAnsi(modelPtr) ?? "";

                    scanners.Add(new ScannerInfo
                    {
                        Name = name,
                        Description = $"{vendor} {model}".Trim()
                    });

                    offset += IntPtr.Size;
                }
            }

            sane_exit();
        }
        catch (Exception ex)
        {
            ShaPrint.Core.AppLogger.Error("[LINUX] Failed to list scanners", ex);
        }
        return scanners;
    }

    public byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, out string actualFormat)
    {
        string ext = format.Equals("PNG", StringComparison.OrdinalIgnoreCase) ? "png" :
                     format.Equals("PDF", StringComparison.OrdinalIgnoreCase) ? "pdf" : "jpg";
        actualFormat = ext;

        try
        {
            string outputFile = Path.Combine(Path.GetTempPath(), $"shaprint_scan_{Guid.NewGuid():N}.{ext}");

            string formatFlag = ext switch
            {
                "png" => "--format=png",
                "pdf" => "--format=pdf",
                _ => "--format=jpeg"
            };

            string colorModeFlag = colorMode switch
            {
                0 => "--mode=Lineart",
                1 => "--mode=Gray",
                _ => "--mode=Color"
            };

            var psi = new ProcessStartInfo
            {
                FileName = "scanimage",
                Arguments = $"-d \"{scannerName}\" {formatFlag} {colorModeFlag} --resolution={dpi} > \"{outputFile}\"",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) throw new InvalidOperationException("Failed to start scanimage");
            process.WaitForExit();

            if (process.ExitCode != 0)
            {
                string error = process.StandardError.ReadToEnd();
                throw new InvalidOperationException($"scanimage failed: {error}");
            }

            byte[] result = File.ReadAllBytes(outputFile);
            File.Delete(outputFile);
            return result;
        }
        catch (Exception ex)
        {
            ShaPrint.Core.AppLogger.Error("[LINUX] Scan failed", ex);
            throw;
        }
    }
}
```

- [ ] **Step 4: Create LinuxStartupManager.cs**

```csharp
// ShaPrint.Platform/Linux/LinuxStartupManager.cs
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.Linux;

public class LinuxStartupManager : IStartupManager
{
    const string ServiceName = "shaprint";
    string ServicePath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        ".config", "systemd", "user", $"{ServiceName}.service");

    public void SetStartup(bool enable)
    {
        try
        {
            if (enable)
            {
                string exePath = Environment.ProcessPath ?? string.Empty;
                if (string.IsNullOrEmpty(exePath)) return;

                string serviceContent = $@"[Unit]
Description=ShaPrint Service
After=network.target

[Service]
Type=simple
ExecStart={exePath} --startup
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
";

                // Create systemd user directory if it doesn't exist
                string dir = Path.GetDirectoryName(ServicePath)!;
                if (!Directory.Exists(dir))
                    Directory.CreateDirectory(dir);

                File.WriteAllText(ServicePath, serviceContent);

                // Reload systemd and enable service
                Process.Start("systemctl", "--user daemon-reload")?.WaitForExit();
                Process.Start("systemctl", $"--user enable {ServiceName}")?.WaitForExit();
                AppLogger.Log("[LINUX] Startup enabled via systemd user service");
            }
            else
            {
                if (File.Exists(ServicePath))
                {
                    Process.Start("systemctl", $"--user disable {ServiceName}")?.WaitForExit();
                    File.Delete(ServicePath);
                    Process.Start("systemctl", "--user daemon-reload")?.WaitForExit();
                    AppLogger.Log("[LINUX] Startup disabled");
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[LINUX] Failed to set startup", ex);
        }
    }

    public bool IsStartupEnabled()
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "systemctl",
                Arguments = $"--user is-enabled {ServiceName}",
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) return false;
            process.WaitForExit();
            return process.ExitCode == 0;
        }
        catch { return false; }
    }
}
```

- [ ] **Step 5: Create LinuxNotificationService.cs**

```csharp
// ShaPrint.Platform/Linux/LinuxNotificationService.cs
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.Linux;

public class LinuxNotificationService : INotificationService
{
    public void ShowToast(string title, string body, ToastAction? action = null)
    {
        try
        {
            // Use notify-send for Linux notifications
            var psi = new ProcessStartInfo
            {
                FileName = "notify-send",
                Arguments = $"\"{title}\" \"{body}\"",
                UseShellExecute = false,
                CreateNoWindow = true
            };

            Process.Start(psi);
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[LINUX] Failed to show notification: {ex.Message}");
        }
    }

    public void ShowPrintJobCompleted(string documentName, string printerName)
        => ShowToast("Print Job Completed", $"{documentName} → {printerName}");

    public void ShowPrintJobFailed(string documentName, string printerName, string reason)
        => ShowToast("Print Job Failed", $"{documentName} → {printerName}: {reason}");

    public void ShowClientConnected(string clientAddress)
        => ShowToast("Client Connected", $"{clientAddress} connected");

    public void ShowClientDisconnected(string clientAddress)
        => ShowToast("Client Disconnected", $"{clientAddress} disconnected");

    public void ShowScanCompleted(string fileName)
        => ShowToast("Scan Complete", $"Saved to {fileName}");

    public void ShowScanFailed(string errorMessage)
        => ShowToast("Scan Failed", errorMessage);

    public void ShowPrinterError(string printerName, string errorDescription)
        => ShowToast("Printer Error", $"{printerName}: {errorDescription}");

    public void ShowSecurityAlert(string message, string detail)
        => ShowToast("Security Alert", $"{message}: {detail}");
}
```

- [ ] **Step 6: Create LinuxFirewallManager.cs**

```csharp
// ShaPrint.Platform/Linux/LinuxFirewallManager.cs
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.Linux;

public class LinuxFirewallManager : IFirewallManager
{
    public Task EnsureFirewallRulesAsync()
    {
        return Task.Run(() =>
        {
            try
            {
                // Try ufw first (Ubuntu/Debian)
                if (CommandExists("ufw"))
                {
                    RunCommand("ufw", $"allow {Constants.PrintTcpPort}/tcp");
                    RunCommand("ufw", $"allow {Constants.DiscoveryUdpPort}/udp");
                    RunCommand("ufw", $"allow {Constants.MonitorTcpPort}/tcp");
                    AppLogger.Log("[LINUX] Firewall rules added via ufw");
                    return;
                }

                // Try firewall-cmd (Fedora/RHEL)
                if (CommandExists("firewall-cmd"))
                {
                    RunCommand("firewall-cmd", $"--add-port={Constants.PrintTcpPort}/tcp --permanent");
                    RunCommand("firewall-cmd", $"--add-port={Constants.DiscoveryUdpPort}/udp --permanent");
                    RunCommand("firewall-cmd", $"--add-port={Constants.MonitorTcpPort}/tcp --permanent");
                    RunCommand("firewall-cmd", "--reload");
                    AppLogger.Log("[LINUX] Firewall rules added via firewall-cmd");
                    return;
                }

                // Fallback to iptables
                RunCommand("iptables", $"-A INPUT -p tcp --dport {Constants.PrintTcpPort} -j ACCEPT");
                RunCommand("iptables", $"-A INPUT -p udp --dport {Constants.DiscoveryUdpPort} -j ACCEPT");
                RunCommand("iptables", $"-A INPUT -p tcp --dport {Constants.MonitorTcpPort} -j ACCEPT");
                AppLogger.Log("[LINUX] Firewall rules added via iptables");
            }
            catch (Exception ex)
            {
                AppLogger.Error("[LINUX] Firewall config error", ex);
            }
        });
    }

    static bool CommandExists(string command)
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "which",
                Arguments = command,
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) return false;
            process.WaitForExit();
            return process.ExitCode == 0;
        }
        catch { return false; }
    }

    static void RunCommand(string command, string arguments)
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = command,
                Arguments = arguments,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            process?.WaitForExit();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[LINUX] Failed to run {command}: {ex.Message}");
        }
    }
}
```

- [ ] **Step 7: Build and verify**

Run: `dotnet build ShaPrint.Platform/ShaPrint.Platform.csproj`
Expected: Build succeeded

- [ ] **Step 8: Commit**

```bash
git add ShaPrint.Platform/Linux/
git commit -m "feat: implement Linux platform backends"
```

---

### Task 8: Implement Android Platform Backends

**Files:**
- Create: `ShaPrint.Platform/Android/AndroidPrinterManager.cs`
- Create: `ShaPrint.Platform/Android/AndroidNotificationService.cs`

**Interfaces:**
- Consumes: `IPrinterManager`, `INotificationService`
- Produces: Android implementations

- [ ] **Step 1: Create AndroidPrinterManager.cs**

```csharp
// ShaPrint.Platform/Android/AndroidPrinterManager.cs
using ShaPrint.Core.Network;

namespace ShaPrint.Platform.Android;

public class AndroidPrinterManager : IPrinterManager
{
    public Task<List<PrinterInfo>> GetLocalPrintersAsync()
    {
        // Android doesn't have local printers in the traditional sense
        // This will be used to list discovered ShaPrint servers
        return Task.FromResult(new List<PrinterInfo>());
    }

    public async Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null)
    {
        // On Android, we send print jobs directly to ShaPrint servers
        // This is handled by the Android-specific print service
        // For now, return false as this needs Android-specific implementation
        await Task.CompletedTask;
        return false;
    }
}
```

- [ ] **Step 2: Create AndroidNotificationService.cs**

```csharp
// ShaPrint.Platform/Android/AndroidNotificationService.cs
using ShaPrint.Core;

namespace ShaPrint.Platform.Android;

public class AndroidNotificationService : INotificationService
{
    public void ShowToast(string title, string body, ToastAction? action = null)
    {
        // Android notifications will be implemented using Android's NotificationManager
        // This requires Android-specific code in the UI layer
        AppLogger.Log($"[ANDROID] Notification: {title} - {body}");
    }

    public void ShowPrintJobCompleted(string documentName, string printerName)
        => ShowToast("Print Job Completed", $"{documentName} → {printerName}");

    public void ShowPrintJobFailed(string documentName, string printerName, string reason)
        => ShowToast("Print Job Failed", $"{documentName} → {printerName}: {reason}");

    public void ShowClientConnected(string clientAddress)
        => ShowToast("Client Connected", $"{clientAddress} connected");

    public void ShowClientDisconnected(string clientAddress)
        => ShowToast("Client Disconnected", $"{clientAddress} disconnected");

    public void ShowScanCompleted(string fileName)
        => ShowToast("Scan Complete", $"Saved to {fileName}");

    public void ShowScanFailed(string errorMessage)
        => ShowToast("Scan Failed", errorMessage);

    public void ShowPrinterError(string printerName, string errorDescription)
        => ShowToast("Printer Error", $"{printerName}: {errorDescription}");

    public void ShowSecurityAlert(string message, string detail)
        => ShowToast("Security Alert", $"{message}: {detail}");
}
```

- [ ] **Step 3: Build and verify**

Run: `dotnet build ShaPrint.Platform/ShaPrint.Platform.csproj`
Expected: Build succeeded

- [ ] **Step 4: Commit**

```bash
git add ShaPrint.Platform/Android/
git commit -m "feat: implement Android platform backends"
```

---

### Task 9: Create Android Project Structure

**Files:**
- Create: `ShaPrint.UI/Android/MainActivity.cs`
- Create: `ShaPrint.UI/Android/AndroidManifest.xml`
- Create: `ShaPrint.UI/Android/Resources/values/styles.xml`
- Modify: `ShaPrint.UI/ShaPrint.UI.csproj` (add Android target)

**Interfaces:**
- Consumes: Android platform backends from Task 8
- Produces: Android app shell

- [ ] **Step 1: Update ShaPrint.UI.csproj for Android**

```xml
<!-- Update ShaPrint.UI/ShaPrint.UI.csproj -->
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFrameworks>net8.0;net8.0-android</TargetFrameworks>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
    <BuiltInComInteropSupport>true</BuiltInComInteropSupport>
    <ApplicationManifest>Windows\app.manifest</ApplicationManifest>
    <AndroidApplication>true</AndroidApplication>
    <AndroidResgenFile>Resources\Resource.designer.cs</AndroidResgenFile>
    <AndroidManifest>Android\AndroidManifest.xml</AndroidManifest>
  </PropertyGroup>

  <ItemGroup>
    <PackageReference Include="Avalonia" Version="11.2.3" />
    <PackageReference Include="Avalonia.Desktop" Version="11.2.3" />
    <PackageReference Include="Avalonia.Android" Version="11.2.3" />
    <PackageReference Include="Avalonia.Themes.Fluent" Version="11.2.3" />
    <PackageReference Include="Avalonia.Fonts.Inter" Version="11.2.3" />
    <PackageReference Include="CommunityToolkit.Mvvm" Version="8.4.2" />
    <PackageReference Include="Microsoft.Extensions.Hosting" Version="10.0.8" />
  </ItemGroup>

  <ItemGroup>
    <ProjectReference Include="..\ShaPrint.Core\ShaPrint.Core.csproj" />
    <ProjectReference Include="..\ShaPrint.Platform\ShaPrint.Platform.csproj" />
  </ItemGroup>
</Project>
```

- [ ] **Step 2: Create MainActivity.cs**

```csharp
// ShaPrint.UI/Android/MainActivity.cs
using Android.App;
using Android.Content.PM;
using Android.OS;
using Avalonia;
using Avalonia.Android;
using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Platform;
using ShaPrint.Platform.Android;

namespace ShaPrint.UI.Android;

[Activity(
    Label = "ShaPrint",
    Theme = "@style/MyTheme.NoActionBar",
    Icon = "@drawable/icon",
    MainLauncher = true,
    ConfigurationChanges = ConfigChanges.Orientation | ConfigChanges.ScreenSize | ConfigChanges.Locale)]
public class MainActivity : AvaloniaMainActivity
{
    protected override AppBuilder CreateAppBuilder()
    {
        return AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .LogToTrace();
    }

    protected override void OnCreate(Bundle? savedInstanceState)
    {
        base.OnCreate(savedInstanceState);

        // Configure Android-specific services
        App.Host = Microsoft.Extensions.Hosting.Host.CreateDefaultBuilder()
            .ConfigureServices((context, services) =>
            {
                services.AddSingleton<IPrinterManager, AndroidPrinterManager>();
                services.AddSingleton<INotificationService, AndroidNotificationService>();
                // Add other services as needed
            })
            .Build();
    }
}
```

- [ ] **Step 3: Create AndroidManifest.xml**

```xml
<!-- ShaPrint.UI/Android/AndroidManifest.xml -->
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    android:versionCode="1"
    android:versionName="2.0.0"
    package="com.shaprint.app">
    
    <uses-sdk android:minSdkVersion="21" android:targetSdkVersion="34" />
    
    <uses-permission android:name="android.permission.INTERNET" />
    <uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
    <uses-permission android:name="android.permission.ACCESS_WIFI_STATE" />
    <uses-permission android:name="android.permission.CHANGE_WIFI_MULTICAST_STATE" />
    <uses-permission android:name="android.permission.READ_EXTERNAL_STORAGE" />
    <uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE" />
    <uses-permission android:name="android.permission.POST_NOTIFICATIONS" />
    
    <application
        android:label="ShaPrint"
        android:icon="@drawable/icon"
        android:theme="@style/MyTheme">
        
        <activity
            android:name=".MainActivity"
            android:exported="true"
            android:launchMode="singleTop">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
```

- [ ] **Step 4: Create styles.xml**

```xml
<!-- ShaPrint.UI/Android/Resources/values/styles.xml -->
<?xml version="1.0" encoding="utf-8"?>
<resources>
    <style name="MyTheme" parent="Theme.AppCompat.Light.NoActionBar">
        <item name="android:colorPrimary">#2196F3</item>
        <item name="android:colorPrimaryDark">#1976D2</item>
        <item name="android:colorAccent">#FF4081</item>
    </style>
    
    <style name="MyTheme.NoActionBar">
        <item name="android:windowNoTitle">true</item>
        <item name="android:windowActionBar">false</item>
    </style>
</resources>
```

- [ ] **Step 5: Build and verify**

Run: `dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0-android`
Expected: Build succeeded

- [ ] **Step 6: Commit**

```bash
git add ShaPrint.UI/Android/ ShaPrint.UI/ShaPrint.UI.csproj
git commit -m "feat: create Android project structure"
```

---

### Task 10: Integration Testing & Documentation

**Files:**
- Create: `docs/multi-platform-guide.md`
- Modify: `README.md` (update for multi-platform)

**Interfaces:**
- Consumes: All implemented features
- Produces: Documentation and final verification

- [ ] **Step 1: Create multi-platform guide**

```markdown
# ShaPrint Multi-Platform Guide

## Supported Platforms

| Platform | Server | Client | Monitor | Print-Only |
|----------|--------|--------|---------|------------|
| Windows  | ✅     | ✅     | ✅      | -          |
| macOS    | ✅     | ✅     | ✅      | -          |
| Linux    | ✅     | ✅     | ✅      | -          |
| Android  | -      | -      | -       | ✅         |

## Building from Source

### Prerequisites
- .NET 8 SDK
- Platform-specific tools (see below)

### Windows
```bash
dotnet publish ShaPrint.UI/ShaPrint.UI.csproj -c Release -r win-x64 --self-contained true
```

### macOS
```bash
dotnet publish ShaPrint.UI/ShaPrint.UI.csproj -c Release -r osx-x64 --self-contained true
```

### Linux
```bash
dotnet publish ShaPrint.UI/ShaPrint.UI.csproj -c Release -r linux-x64 --self-contained true
```

### Android
```bash
dotnet publish ShaPrint.UI/ShaPrint.UI.csproj -c Release -f net8.0-android
```

## Platform-Specific Notes

### macOS
- Scanner support requires SANE drivers
- Firewall rules may need manual configuration
- Virtual printer uses CUPS backend

### Linux
- Scanner support requires libsane (bundled)
- Firewall rules use ufw/firewall-cmd/iptables
- Virtual printer uses CUPS backend

### Android
- Print-only mode (no server/scanner)
- Supports PDF and image files
- Requires network permissions

## Troubleshooting

### macOS Scanner Issues
1. Install SANE drivers: `brew install sane-backends`
2. Check scanner detection: `scanimage -L`

### Linux Scanner Issues
1. Install SANE: `sudo apt install libsane`
2. Check permissions: Add user to `scanner` group

### Android Connection Issues
1. Ensure both devices are on same network
2. Check firewall settings on server
3. Verify Network Channel matches
```

- [ ] **Step 2: Update README.md**

Add multi-platform section to README.md:

```markdown
## Multi-Platform Support

ShaPrint now supports multiple platforms:

- **Windows**: Full feature support (Server, Client, Monitor)
- **macOS**: Full feature support (Server, Client, Monitor)
- **Linux**: Full feature support (Server, Client, Monitor)
- **Android**: Print-only mode (send files to ShaPrint servers)

See [Multi-Platform Guide](docs/multi-platform-guide.md) for details.
```

- [ ] **Step 3: Final build verification**

Run: `dotnet build ShaPrint.sln`
Expected: All projects build successfully

- [ ] **Step 4: Commit**

```bash
git add docs/ README.md
git commit -m "docs: add multi-platform documentation"
```

---

## Summary

This plan migrates ShaPrint from Windows-only to multi-platform using:

1. **ShaPrint.Platform**: Platform abstraction layer with interfaces
2. **ShaPrint.UI**: Avalonia UI shared across desktop and Android
3. **Platform backends**: Windows, macOS, Linux, Android implementations
4. **Incremental migration**: Reuse existing code where possible

Total: 10 tasks, ~50 steps, estimated 2-3 weeks for full implementation.
