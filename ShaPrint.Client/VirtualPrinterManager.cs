using System;
using System.Diagnostics;
using System.Threading.Tasks;

namespace ShaPrint.Client
{
    public static class VirtualPrinterManager
    {
        public static async Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string pipeName, string driverName)
        {
            return await Task.Run(() =>
            {
                try
                {
                    var portResult = RunPowerShell($"Add-PrinterPort -Name '{pipeName}'");
                    if (!portResult.Success && !portResult.ErrorMessage.Contains("already exists"))
                    {
                        ShaPrint.Core.AppLogger.Log("[CLIENT] Add-PrinterPort warning: " + portResult.ErrorMessage);
                    }

                    // Try adding the driver if it's an inbox driver (might fail if it's external, but we try)
                    RunPowerShell($"Add-PrinterDriver -Name '{driverName}'");
                    
                    var addPrinterResult = RunPowerShell($"Add-Printer -Name '{virtualPrinterName}' -DriverName '{driverName}' -PortName '{pipeName}'");
                    if (!addPrinterResult.Success)
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] Warning: Failed to install with Native Driver '{driverName}'. Falling back to 'Generic / Text Only'.");
                        RunPowerShell($"Add-PrinterDriver -Name 'Generic / Text Only'");
                        addPrinterResult = RunPowerShell($"Add-Printer -Name '{virtualPrinterName}' -DriverName 'Generic / Text Only' -PortName '{pipeName}'");
                    }

                    if (addPrinterResult.Success)
                    {
                        // Disable Bidirectional Support (BIDI) to prevent Windows Spooler / Word from hanging while "Connecting to printer"
                        ShaPrint.Core.AppLogger.Log("[CLIENT] Disabling Bidirectional Support on the Virtual Printer to prevent UI hanging...");
                        RunPowerShell($"$printer = Get-WmiObject -Class Win32_Printer | Where-Object {{ $_.Name -eq '{virtualPrinterName}' }}; if ($printer) {{ $printer.EnableBIDI = $false; $printer.Put() }}");
                        
                        return (true, string.Empty);
                    }
                    return (false, "All driver installation attempts failed. Last error: " + addPrinterResult.ErrorMessage);
                }
                catch (Exception ex)
                {
                    return (false, "Exception: " + ex.Message);
                }
            });
        }

        public static async Task<bool> RemovePrinterAsync(string printerName, string pipeName)
        {
            return await Task.Run(() =>
            {
                try
                {
                    RunPowerShell($"Remove-Printer -Name '{printerName}'");
                    RunPowerShell($"Remove-PrinterPort -Name '{pipeName}'");
                    return true;
                }
                catch
                {
                    return false;
                }
            });
        }

        public static bool CheckPrinterExists(string printerName)
        {
            var result = RunPowerShell($"Get-Printer -Name '{printerName}'");
            return result.Success;
        }

        private static (bool Success, string ErrorMessage) RunPowerShell(string script)
        {
            var psi = new ProcessStartInfo
            {
                FileName = "powershell.exe",
                Arguments = $"-NoProfile -ExecutionPolicy Bypass -Command \"{script}\"",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null) return (false, "Failed to start powershell.");
            
            process.WaitForExit();
            string errors = process.StandardError.ReadToEnd();
            return (process.ExitCode == 0, errors);
        }
    }
}
