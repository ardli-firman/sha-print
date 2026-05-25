using System;
using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.WpfApp.Utils
{
    public static class StartupManager
    {
        private const string TaskName = "ShaPrint_Startup";

        public static void SetStartup(bool enable)
        {
            try
            {
                string exePath = Environment.ProcessPath ?? string.Empty;
                if (string.IsNullOrEmpty(exePath)) return;

                if (enable)
                {
                    var psi = new ProcessStartInfo
                    {
                        FileName = "schtasks.exe",
                        Arguments = $"/create /tn \"{TaskName}\" /tr \"\\\"{exePath}\\\" --startup\" /sc onlogon /rl highest /f",
                        CreateNoWindow = true,
                        UseShellExecute = false
                    };
                    using var p = Process.Start(psi);
                    p?.WaitForExit();
                    AppLogger.Log("[SYSTEM] Enabled Run on Windows Startup via Task Scheduler (Highest Privileges).");
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
                    AppLogger.Log("[SYSTEM] Disabled Run on Windows Startup via Task Scheduler.");
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("[SYSTEM] Failed to change startup settings: " + ex.Message);
            }
        }

        public static bool IsStartupEnabled()
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
            catch
            {
                return false;
            }
        }
    }
}
