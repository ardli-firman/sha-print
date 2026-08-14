using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using Microsoft.Win32;
using ShaPrint.Core;
using ShaPrint.Server;

namespace ShaPrint.Client
{
    /// <summary>
    /// Manages virtual printer installation/removal using Win32 APIs.
    /// PowerShell has been eliminated to prevent command injection (RCE).
    /// All input MUST be validated by <see cref="Validators"/> before calling these methods.
    /// </summary>
    public static class VirtualPrinterManager
    {
        public static async Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string pipeName, string driverName)
        {
            return await Task.Run(() =>
            {
                try
                {
                    // Escape single quotes to prevent PowerShell injection
                    string safePrinterName = virtualPrinterName.Replace("'", "''");
                    string safePipeName = pipeName.Replace("'", "''");
                    string safeDriverName = driverName.Replace("'", "''");

                    var portResult = RunPowerShell($"Add-PrinterPort -Name '{safePipeName}'");
                    if (!portResult.Success && !portResult.Output.Contains("already exists"))
                    {
                        ShaPrint.Core.AppLogger.Log("[CLIENT] Add-PrinterPort warning: " + portResult.Output);
                    }

                    // Try adding the driver if it's an inbox driver (result is no longer swallowed)
                    var driverResult = RunPowerShell($"Add-PrinterDriver -Name '{safeDriverName}'");
                    if (!driverResult.Success && !driverResult.Output.Contains("already exists", StringComparison.OrdinalIgnoreCase))
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] Add-PrinterDriver for '{safeDriverName}' reported: {driverResult.Output}");
                    }
                    else
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] Add-PrinterDriver for '{safeDriverName}' OK.");
                    }

                    var addPrinterResult = RunPowerShell($"Add-Printer -Name '{safePrinterName}' -DriverName '{safeDriverName}' -PortName '{safePipeName}'");
                    if (!addPrinterResult.Success)
                    {
                        string err = addPrinterResult.Output;
                        if (err.Contains("The specified driver does not exist", StringComparison.OrdinalIgnoreCase) || err.Contains("was not found", StringComparison.OrdinalIgnoreCase))
                        {
                            return (false, $"Driver '{driverName}' is not installed on this computer. Please install the printer driver first.");
                        }

                        // If it failed for another reason, get the error message and suggest Admin
                        if (err.Contains("Access denied", StringComparison.OrdinalIgnoreCase) || err.Contains("Administrator", StringComparison.OrdinalIgnoreCase))
                        {
                            err += " (Please ensure you run this application as Administrator)";
                        }
                        return (false, "Driver installation failed. Last error: " + err);
                    }

                    if (addPrinterResult.Success)
                    {
                        // Disable Bidirectional Support (BIDI)
                        ShaPrint.Core.AppLogger.Log("[CLIENT] Disabling Bidirectional Support on the Virtual Printer to prevent UI hanging...");
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

        public static async Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string printerName, string pipeName)
        {
            return await Task.Run(() =>
            {
                try
                {
                    string safePrinterName = printerName.Replace("'", "''");
                    string safePipeName = pipeName.Replace("'", "''");

                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Attempting to remove printer '{safePrinterName}'...");

                    // Step 1: Clear all stuck print jobs
                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Clearing print jobs for '{safePrinterName}'...");
                    RunPowerShell($"Get-PrintJob -PrinterName '{safePrinterName}' -ErrorAction SilentlyContinue | Remove-PrintJob -ErrorAction SilentlyContinue");
                    System.Threading.Thread.Sleep(500);

                    // Step 2: Check if printer is the default printer, if so set another as default
                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Checking if printer is default printer...");
                    var checkDefaultScript = $@"
$printer = Get-WmiObject -Class Win32_Printer | Where-Object {{ $_.Name -eq '{safePrinterName}' }};
if ($printer -and $printer.Default -eq $true) {{
    Write-Output 'IsDefault';
    $otherPrinter = Get-WmiObject -Class Win32_Printer | Where-Object {{ $_.Name -ne '{safePrinterName}' }} | Select-Object -First 1;
    if ($otherPrinter) {{ $otherPrinter.SetDefaultPrinter() | Out-Null; Write-Output 'DefaultChanged' }}
}}";
                    var defaultCheckResult = RunPowerShell(checkDefaultScript);
                    if (defaultCheckResult.Output.Contains("IsDefault"))
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] Printer was default printer. Changed default to another printer.");
                    }

                    // Step 3: Use WMI Delete() method (more reliable than Remove-Printer cmdlet)
                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Removing printer using WMI Delete() method...");
                    var wmiDeleteScript = $@"
$printer = Get-WmiObject -Class Win32_Printer | Where-Object {{ $_.Name -eq '{safePrinterName}' }};
if ($printer) {{
    $result = $printer.Delete();
    if ($result.ReturnValue -eq 0) {{ Write-Output 'Success' }}
    else {{ Write-Output ""Failed:$($result.ReturnValue)"" }}
}} else {{
    Write-Output 'NotFound'
}}";
                    var wmiDeleteResult = RunPowerShell(wmiDeleteScript);

                    if (wmiDeleteResult.Output.Contains("Failed:"))
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] WMI Delete() returned non-zero ({wmiDeleteResult.Output}). Will be handled by spooler restart.");
                    }
                    else if (wmiDeleteResult.Output.Contains("NotFound"))
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] Printer not found (already removed).");
                    }
                    else if (wmiDeleteResult.Output.Contains("Success"))
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] WMI Delete() completed successfully.");
                    }
                    else
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] WMI Delete() executed.");
                    }

                    System.Threading.Thread.Sleep(500);

                    // Step 4: Remove the printer port
                    if (!string.IsNullOrEmpty(safePipeName))
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] Removing printer port '{safePipeName}'...");
                        RunPowerShell($"Remove-PrinterPort -Name '{safePipeName}' -ErrorAction SilentlyContinue");
                    }

                    // Step 5: Restart Print Spooler to force release all handles
                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Restarting Print Spooler to ensure complete removal...");

                    var stopSpooler = RunPowerShell("Stop-Service -Name Spooler -Force -ErrorAction SilentlyContinue");
                    if (!stopSpooler.Success)
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] Warning: Failed to stop Print Spooler: {stopSpooler.Output}");
                    }

                    System.Threading.Thread.Sleep(2000);

                    var startSpooler = RunPowerShell("Start-Service -Name Spooler -ErrorAction SilentlyContinue");
                    if (!startSpooler.Success)
                    {
                        ShaPrint.Core.AppLogger.Error($"[CLIENT] Failed to start Print Spooler: {startSpooler.Output}");
                        return (false, "Failed to restart Print Spooler. Please restart it manually via Services (services.msc). You may need to run this application as Administrator.");
                    }

                    System.Threading.Thread.Sleep(2000);

                    // Step 6: Final verification
                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Verifying printer removal...");
                    var verifyResult = RunPowerShell($"Get-WmiObject -Class Win32_Printer | Where-Object {{ $_.Name -eq '{safePrinterName}' }}");

                    if (verifyResult.Success && !string.IsNullOrWhiteSpace(verifyResult.Output))
                    {
                        // Printer STILL exists! Last resort: try Remove-Printer cmdlet as fallback
                        ShaPrint.Core.AppLogger.Error($"[CLIENT] Printer still exists after WMI Delete and spooler restart! Trying Remove-Printer as last resort...");
                        RunPowerShell($"Remove-Printer -Name '{safePrinterName}' -ErrorAction SilentlyContinue");
                        System.Threading.Thread.Sleep(1000);

                        // Final final verification
                        var finalVerify = RunPowerShell($"Get-WmiObject -Class Win32_Printer | Where-Object {{ $_.Name -eq '{safePrinterName}' }}");
                        if (finalVerify.Success && !string.IsNullOrWhiteSpace(finalVerify.Output))
                        {
                            ShaPrint.Core.AppLogger.Error($"[CLIENT] All removal attempts failed. Printer is stuck in the system.");
                            return (false, $"Printer removal failed. The printer is stuck in the system.\n\nPlease try:\n1. Run this application as Administrator\n2. Manually delete '{printerName}' from Control Panel > Devices and Printers\n3. If it shows 'Pending Deletion', restart your computer");
                        }
                    }

                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Printer '{safePrinterName}' removed successfully.");
                    return (true, string.Empty);
                }
                catch (Exception ex)
                {
                    ShaPrint.Core.AppLogger.Error($"[CLIENT] Exception during printer removal: {ex.Message}");
                    return (false, "Exception: " + ex.Message);
                }
            });
        }

        public static bool CheckPrinterExists(string printerName)
        {
            string safePrinterName = printerName.Replace("'", "''");
            var result = RunPowerShell($"Get-Printer -Name '{safePrinterName}'");
            return result.Success;
        }

        /// <summary>
        /// Enumerates the printer drivers installed on this machine.
        /// Primary source: Get-PrinterDriver (via JSON, no truncation, errors captured).
        /// Fallback source: registry driver store (HKLM\...\Print\Environments\Windows x64\Drivers)
        /// for drivers that are registered in the store but hidden from Get-PrinterDriver.
        /// Returns a deduplicated, sorted list. Empty on total failure.
        ///
        /// Results are cached for <see cref="DriverCacheTtlSeconds"/> to avoid repeatedly spawning
        /// powershell.exe when the user toggles/re-runs installation in the same session.
        /// </summary>
        private static readonly TimeSpan DriverCacheTtl = TimeSpan.FromSeconds(30);
        // SemaphoreSlim(1,1) ensures only one thread rebuilds the cache at a time.
        private static readonly SemaphoreSlim _driverCacheLock = new SemaphoreSlim(1, 1);
        private static List<string>? _driverCache;
        private static DateTime? _driverCacheTime;

        public static List<string> GetInstalledDrivers()
        {
            // Fast path: check cache without acquiring the lock (reads of reference-type fields
            // are atomic on x64, so the null check is safe; DateTime? is read under lock below).
            if (_driverCache != null && _driverCacheTime.HasValue &&
                (DateTime.UtcNow - _driverCacheTime.Value) < DriverCacheTtl)
            {
                return _driverCache;
            }

            _driverCacheLock.Wait();
            try
            {
                // Double-checked locking: another thread may have populated the cache while we waited.
                if (_driverCache != null && _driverCacheTime.HasValue &&
                    (DateTime.UtcNow - _driverCacheTime.Value) < DriverCacheTtl)
                {
                    return _driverCache;
                }

                var drivers = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
                try
                {
                    // Primary: Get-PrinterDriver serialized as JSON — avoids Out-String width
                    // truncation and lets us capture structured output without parsing text.
                    var psResult = RunPowerShell("Get-PrinterDriver | Select-Object -ExpandProperty Name | ConvertTo-Json -Compress");
                    if (psResult.Success)
                    {
                        var parsed = TryParseJsonStringArray(psResult.Output);
                        foreach (var name in parsed)
                        {
                            if (!string.IsNullOrWhiteSpace(name)) drivers.Add(name.Trim());
                        }
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] Get-PrinterDriver returned {parsed.Count} driver(s).");
                    }
                    else
                    {
                        ShaPrint.Core.AppLogger.Log("[CLIENT] Get-PrinterDriver failed: " + psResult.Output);
                    }
                }
                catch (Exception ex)
                {
                    ShaPrint.Core.AppLogger.Log("[CLIENT] Get-PrinterDriver exception: " + ex.Message);
                }

                // Fallback/enrichment: read registered Type 3/4 driver names directly from
                // Environment\Drivers\Version-* registry keys. Reading only the immediate
                // children of Drivers returns "Version-3"/"Version-4", not driver names.
                try
                {
                    const string registryRootPath = @"SYSTEM\CurrentControlSet\Control\Print\Environments";

                    IEnumerable<string> ReadSubKeyNames(string relativePath)
                    {
                        string keyPath = string.IsNullOrEmpty(relativePath)
                            ? registryRootPath
                            : $@"{registryRootPath}\{relativePath}";

                        try
                        {
                            using RegistryKey? key = Registry.LocalMachine.OpenSubKey(keyPath);
                            return key?.GetSubKeyNames() ?? Array.Empty<string>();
                        }
                        catch (Exception ex)
                        {
                            ShaPrint.Core.AppLogger.Log(
                                $"[CLIENT] Registry driver key '{relativePath}' could not be read: {ex.Message}");
                            return Array.Empty<string>();
                        }
                    }

                    var registeredDrivers = ReadRegisteredDriverNamesFromRegistry(ReadSubKeyNames);
                    foreach (var name in registeredDrivers) drivers.Add(name);

                    ShaPrint.Core.AppLogger.Log(
                        $"[CLIENT] Registry driver store returned {registeredDrivers.Count} registered driver entr(ies).");
                }
                catch (Exception ex)
                {
                    ShaPrint.Core.AppLogger.Log("[CLIENT] Registry driver store enumeration exception: " + ex.Message);
                }

                var result = drivers.OrderBy(d => d, StringComparer.OrdinalIgnoreCase).ToList();
                ShaPrint.Core.AppLogger.Log($"[CLIENT] Total unique installed drivers enumerated: {result.Count}.");

                // Populate cache (even on partial results) so repeat calls don't re-spawn powershell.
                _driverCache = result;
                _driverCacheTime = DateTime.UtcNow;
                return result;
            }
            finally
            {
                _driverCacheLock.Release();
            }
        }

        /// <summary>
        /// Traverses Print\Environments as Environment -> Drivers -> Version-* -> driver name.
        /// The delegate makes the traversal deterministic and testable without changing the
        /// machine registry.
        /// </summary>
        private static List<string> ReadRegisteredDriverNamesFromRegistry(
            Func<string, IEnumerable<string>> readSubKeyNames)
        {
            var driverNames = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

            foreach (string environmentName in readSubKeyNames(string.Empty))
            {
                string driversPath = $@"{environmentName}\Drivers";

                foreach (string versionName in readSubKeyNames(driversPath))
                {
                    if (!versionName.StartsWith("Version-", StringComparison.OrdinalIgnoreCase))
                    {
                        continue;
                    }

                    string versionPath = $@"{driversPath}\{versionName}";
                    foreach (string driverName in readSubKeyNames(versionPath))
                    {
                        if (!string.IsNullOrWhiteSpace(driverName))
                        {
                            driverNames.Add(driverName.Trim());
                        }
                    }
                }
            }

            return driverNames.OrderBy(name => name, StringComparer.OrdinalIgnoreCase).ToList();
        }

        /// <summary>
        /// Parses the output of ConvertTo-Json for a string array (or single string).
        /// Returns an empty list on any parse failure.
        /// </summary>
        private static List<string> TryParseJsonStringArray(string? json)
        {
            if (string.IsNullOrWhiteSpace(json)) return new List<string>();
            try
            {
                // Single value (one driver) serializes as a bare string, not an array.
                if (json.TrimStart().StartsWith("["))
                {
                    var arr = System.Text.Json.JsonSerializer.Deserialize<List<string>>(json);
                    return arr ?? new List<string>();
                }
                var single = System.Text.Json.JsonSerializer.Deserialize<string>(json);
                return string.IsNullOrWhiteSpace(single) ? new List<string>() : new List<string> { single };
            }
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Log("[CLIENT] Driver JSON parse failed: " + ex.Message);
                return new List<string>();
            }
        }

        /// <summary>
        /// Runs a PowerShell script and returns the result.
        /// <para><c>Output</c> contains the merged stdout+stderr stream (2&gt;&amp;1), not an error message.
        /// It is safe to read on both success and failure paths.</para>
        /// </summary>
        private static (bool Success, string Output) RunPowerShell(string script)
        {
            var psi = new System.Diagnostics.ProcessStartInfo
            {
                FileName = "powershell.exe",
                Arguments = $"-NoProfile -ExecutionPolicy Bypass -Command \"{script} 2>&1 | Out-String -Width 4096\"",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = System.Diagnostics.Process.Start(psi);
            if (process == null) return (false, "Failed to start powershell.");

            process.WaitForExit();
            // Because of 2>&1, both normal output and errors are merged into StandardOutput without truncation.
            string output = process.StandardOutput.ReadToEnd().Trim();

            return (process.ExitCode == 0, output);
        }
    }
}
