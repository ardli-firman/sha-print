using System;
using System.IO;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Abstractions;

namespace ShaPrint.WpfApp.Services.Client
{
    /// <summary>
    /// Result of a driver installation attempt.
    /// </summary>
    public class DriverInstallResult
    {
        public bool Success { get; set; }
        public string? ErrorMessage { get; set; }
        public string? InstalledDriverName { get; set; }
    }

    /// <summary>
    /// Installs printer drivers from exported driver packages.
    /// 
    /// Install chain (per plan Q3):
    ///   1. Add-PrinterDriver -InfPath <path.inf> (third-party)
    ///   2. pnputil /add-driver <path.inf> /install (fallback)
    ///   3. Add-PrinterDriver -Name <name> (inbox drivers — driver must pre-exist)
    /// 
    /// All errors are surfaced (never swallowed).
    /// </summary>
    public class DriverInstaller
    {
        private readonly IProcessRunner _processRunner;

        public DriverInstaller(IProcessRunner processRunner)
        {
            _processRunner = processRunner;
        }

        /// <summary>
        /// Installs a driver from the given .inf file path.
        /// Tries Add-PrinterDriver -InfPath first, then pnputil fallback, then inbox fallback (H6).
        /// </summary>
        public async Task<DriverInstallResult> InstallDriverFromInfAsync(string infPath, string? driverName = null)
        {
            if (string.IsNullOrEmpty(infPath) || !File.Exists(infPath))
            {
                return new DriverInstallResult
                {
                    Success = false,
                    ErrorMessage = $"Driver .inf file not found: {infPath}"
                };
            }

            AppLogger.Log($"[DRIVER_INSTALL] Installing driver from: {infPath} (target driver='{driverName ?? "<any>"}')");

            string safeInfPath = infPath.Replace("'", "''");

            // Strategy 1a: Add-PrinterDriver -InfPath -Name (most precise — installs exact model)
            // This is the correct way to install a specific printer driver from an INF that may
            // contain multiple printer models (e.g., a vendor INF with 50+ Epson models).
            if (!string.IsNullOrWhiteSpace(driverName))
            {
                string safeName = driverName.Replace("'", "''");
                AppLogger.Log($"[DRIVER_INSTALL] Strategy 1a: Add-PrinterDriver -InfPath + -Name '{driverName}'");
                var preciseResult = await _processRunner.RunAsync("powershell.exe",
                    $"-NoProfile -ExecutionPolicy Bypass -Command \"Add-PrinterDriver -Name '{safeName}' -InfPath '{safeInfPath}' 2>&1 | Out-String -Width 4096\"",
                    TimeSpan.FromMinutes(2));

                if (preciseResult.Success)
                {
                    AppLogger.Log($"[DRIVER_INSTALL] Strategy 1a succeeded: driver '{driverName}' installed.");
                    return new DriverInstallResult { Success = true, InstalledDriverName = driverName };
                }
                AppLogger.Log($"[DRIVER_INSTALL] Strategy 1a failed: {preciseResult.Output.Trim()}");
            }

            // Strategy 1b: Add-PrinterDriver -InfPath (no name — let Windows pick, works for single-model INFs)
            AppLogger.Log("[DRIVER_INSTALL] Strategy 1b: Add-PrinterDriver -InfPath (no name)");
            var result = await _processRunner.RunAsync("powershell.exe",
                $"-NoProfile -ExecutionPolicy Bypass -Command \"Add-PrinterDriver -InfPath '{safeInfPath}' 2>&1 | Out-String -Width 4096\"",
                TimeSpan.FromMinutes(2));

            if (result.Success)
            {
                AppLogger.Log("[DRIVER_INSTALL] Strategy 1b succeeded.");
                return new DriverInstallResult { Success = true, InstalledDriverName = driverName };
            }

            string addPrinterError = result.Output.Trim();
            AppLogger.Log($"[DRIVER_INSTALL] Strategy 1b failed: {addPrinterError}");

            // Strategy 2: pnputil /add-driver (registers INF into Windows driver store)
            AppLogger.Log("[DRIVER_INSTALL] Strategy 2: pnputil /add-driver");
            var pnputilResult = await _processRunner.RunAsync("pnputil",
                $"/add-driver \"{infPath}\" /install",
                TimeSpan.FromMinutes(2));

            if (pnputilResult.Success)
            {
                AppLogger.Log("[DRIVER_INSTALL] Strategy 2 (pnputil) succeeded.");
                // After pnputil, try Add-PrinterDriver -Name to register with spooler
                if (!string.IsNullOrWhiteSpace(driverName))
                {
                    string safeName = driverName.Replace("'", "''");
                    AppLogger.Log($"[DRIVER_INSTALL] Strategy 2b: Add-PrinterDriver -Name '{driverName}' (post-pnputil)");
                    var postPnpResult = await _processRunner.RunAsync("powershell.exe",
                        $"-NoProfile -ExecutionPolicy Bypass -Command \"Add-PrinterDriver -Name '{safeName}' 2>&1 | Out-String -Width 4096\"",
                        TimeSpan.FromSeconds(30));
                    if (postPnpResult.Success)
                        AppLogger.Log($"[DRIVER_INSTALL] Strategy 2b: spooler registration succeeded.");
                    else
                        AppLogger.Log($"[DRIVER_INSTALL] Strategy 2b: spooler registration failed (may already be registered): {postPnpResult.Output.Trim()}");
                }
                return new DriverInstallResult { Success = true, InstalledDriverName = driverName };
            }

            string pnputilError = pnputilResult.Output.Trim();
            AppLogger.Log($"[DRIVER_INSTALL] Strategy 2 failed: {pnputilError}");

            string combinedError = $"Driver installation failed.\n" +
                $"  Strategy 1a (Add-PrinterDriver -Name + -InfPath): {(string.IsNullOrWhiteSpace(driverName) ? "skipped" : "failed")}\n" +
                $"  Strategy 1b (Add-PrinterDriver -InfPath): {addPrinterError}\n" +
                $"  Strategy 2  (pnputil /add-driver): {pnputilError}";

            // Strategy 3: Inbox driver fallback (H6)
            if (!string.IsNullOrWhiteSpace(driverName))
            {
                AppLogger.Log("[DRIVER_INSTALL] Strategy 3: inbox driver fallback...");
                var inboxResult = await InstallInboxDriverAsync(driverName);
                if (inboxResult.Success)
                {
                    return inboxResult;
                }
                combinedError += $"\n  Strategy 3  (Add-PrinterDriver -Name inbox): {inboxResult.ErrorMessage}";
            }

            AppLogger.Error("[DRIVER_INSTALL] All install strategies failed.");
            return new DriverInstallResult
            {
                Success = false,
                ErrorMessage = combinedError
            };
        }

        /// <summary>
        /// Installs an inbox driver by name (driver must already be in the driver store).
        /// </summary>
        public async Task<DriverInstallResult> InstallInboxDriverAsync(string driverName)
        {
            if (string.IsNullOrWhiteSpace(driverName))
            {
                return new DriverInstallResult
                {
                    Success = false,
                    ErrorMessage = "Driver name cannot be empty."
                };
            }

            string safeName = driverName.Replace("'", "''");
            var result = await _processRunner.RunAsync("powershell.exe",
                $"-NoProfile -ExecutionPolicy Bypass -Command \"Add-PrinterDriver -Name '{safeName}' 2>&1 | Out-String -Width 4096\"",
                TimeSpan.FromSeconds(30));

            if (result.Success || result.Output.Contains("already exists", StringComparison.OrdinalIgnoreCase))
            {
                AppLogger.Log($"[DRIVER_INSTALL] Inbox driver '{driverName}' installed/already present.");
                return new DriverInstallResult { Success = true, InstalledDriverName = driverName };
            }

            return new DriverInstallResult
            {
                Success = false,
                ErrorMessage = $"Inbox driver install failed: {result.Output.Trim()}"
            };
        }
    }
}
