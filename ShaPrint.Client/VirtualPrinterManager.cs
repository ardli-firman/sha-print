using System;
using System.Diagnostics;
using System.Threading.Tasks;

namespace ShaPrint.Client
{
    public static class VirtualPrinterManager
    {
        public static async Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string printerName, string pipeName)
        {
            return await Task.Run(() =>
            {
                try
                {
                    // 1. Add Local Port (Named Pipe)
                    var portResult = RunPowerShell($"Add-PrinterPort -Name '{pipeName}'");
                    if (!portResult.Success)
                    {
                        // Port might already exist, which is fine. But let's log it.
                        Console.WriteLine("Add-PrinterPort warning: " + portResult.ErrorMessage);
                    }

                    // 2. Try adding printer with various common inbox drivers
                    string[] candidateDrivers = new[] 
                    {
                        "Microsoft Print To PDF",
                        "Universal Print Class Driver",
                        "Microsoft Virtual Print Class Driver",
                        "Microsoft IPP Class Driver",
                        "Microsoft XPS Document Writer",
                        "Microsoft XPS Document Writer v4"
                    };

                    string lastError = "No drivers found.";

                    foreach (var driverName in candidateDrivers)
                    {
                        var result = RunPowerShell($"Add-Printer -Name '{printerName}' -DriverName '{driverName}' -PortName '{pipeName}'");
                        if (result.Success)
                        {
                            return (true, string.Empty);
                        }
                        lastError = result.ErrorMessage;
                    }
                    
                    return (false, "All driver installation attempts failed. Last error: " + lastError);
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
