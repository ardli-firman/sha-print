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
        /// Tries Add-PrinterDriver -InfPath first, then pnputil fallback.
        /// </summary>
        public async Task<DriverInstallResult> InstallDriverFromInfAsync(string infPath)
        {
            if (string.IsNullOrEmpty(infPath) || !File.Exists(infPath))
            {
                return new DriverInstallResult
                {
                    Success = false,
                    ErrorMessage = $"Driver .inf file not found: {infPath}"
                };
            }

            AppLogger.Log($"[DRIVER_INSTALL] Installing driver from: {infPath}");

            // Strategy 1: Add-PrinterDriver -InfPath (preferred for third-party)
            string safeInfPath = infPath.Replace("'", "''");
            var result = await _processRunner.RunAsync("powershell.exe",
                $"-NoProfile -ExecutionPolicy Bypass -Command \"Add-PrinterDriver -InfPath '{safeInfPath}' 2>&1 | Out-String -Width 4096\"",
                TimeSpan.FromMinutes(2));

            if (result.Success)
            {
                AppLogger.Log("[DRIVER_INSTALL] Add-PrinterDriver -InfPath succeeded.");
                return new DriverInstallResult { Success = true };
            }

            string addPrinterError = result.Output.Trim();
            AppLogger.Log($"[DRIVER_INSTALL] Add-PrinterDriver -InfPath failed: {addPrinterError}");

            // Strategy 2: pnputil /add-driver (fallback)
            var pnputilResult = await _processRunner.RunAsync("pnputil",
                $"/add-driver \"{infPath}\" /install",
                TimeSpan.FromMinutes(2));

            if (pnputilResult.Success)
            {
                AppLogger.Log("[DRIVER_INSTALL] pnputil /add-driver succeeded.");
                return new DriverInstallResult { Success = true };
            }

            string pnputilError = pnputilResult.Output.Trim();
            AppLogger.Log($"[DRIVER_INSTALL] pnputil /add-driver failed: {pnputilError}");

            // Both strategies failed
            string combinedError = $"Driver installation failed.\n" +
                $"  Add-PrinterDriver -InfPath: {addPrinterError}\n" +
                $"  pnputil /add-driver: {pnputilError}";

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
