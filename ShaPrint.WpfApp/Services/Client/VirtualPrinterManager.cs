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
                    if (!portResult.Success && !portResult.ErrorMessage.Contains("already exists"))
                    {
                        ShaPrint.Core.AppLogger.Log("[CLIENT] Add-PrinterPort warning: " + portResult.ErrorMessage);
                    }

                    // Try adding the driver if it's an inbox driver
                    RunPowerShell($"Add-PrinterDriver -Name '{safeDriverName}'");
                    
                    var addPrinterResult = RunPowerShell($"Add-Printer -Name '{safePrinterName}' -DriverName '{safeDriverName}' -PortName '{safePipeName}'");
                    if (!addPrinterResult.Success)
                    {
                        string err = addPrinterResult.ErrorMessage;
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
                    
                    // Clear any stuck print jobs to prevent "ghost printers" (Pending Deletion)
                    RunPowerShell($"Get-PrintJob -PrinterName '{safePrinterName}' -ErrorAction SilentlyContinue | Remove-PrintJob -ErrorAction SilentlyContinue");

                    var removeResult = RunPowerShell($"Remove-Printer -Name '{safePrinterName}'");
                    
                    if (!string.IsNullOrEmpty(safePipeName))
                    {
                        RunPowerShell($"Remove-PrinterPort -Name '{safePipeName}'");
                    }

                    // If it was already removed, we can consider it a success
                    if (!removeResult.Success && removeResult.ErrorMessage.Contains("was not found"))
                    {
                        return (true, string.Empty);
                    }

                    return (removeResult.Success, removeResult.ErrorMessage);
                }
                catch (Exception ex)
                {
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

        private static (bool Success, string ErrorMessage) RunPowerShell(string script)
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
            // Because of 2>&1, both normal output and errors are merged into StandardOutput without truncation
            string output = process.StandardOutput.ReadToEnd().Trim();
            
            return (process.ExitCode == 0, output);
        }
    }
}
