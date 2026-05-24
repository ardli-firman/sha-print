using System;
using Microsoft.Win32;
using ShaPrint.Core;

namespace ShaPrint.WpfApp.Utils
{
    public static class StartupManager
    {
        private const string AppName = "ShaPrint";

        public static void SetStartup(bool enable)
        {
            try
            {
                string exePath = Environment.ProcessPath ?? string.Empty;
                if (string.IsNullOrEmpty(exePath)) return;

                using (RegistryKey? key = Registry.CurrentUser.OpenSubKey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run", true))
                {
                    if (key != null)
                    {
                        if (enable)
                        {
                            key.SetValue(AppName, $"\"{exePath}\" --startup");
                            AppLogger.Log("[SYSTEM] Enabled Run on Windows Startup.");
                        }
                        else
                        {
                            key.DeleteValue(AppName, false);
                            AppLogger.Log("[SYSTEM] Disabled Run on Windows Startup.");
                        }
                    }
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
                using (RegistryKey? key = Registry.CurrentUser.OpenSubKey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run", false))
                {
                    var val = key?.GetValue(AppName)?.ToString();
                    if (val != null)
                    {
                        if (!val.Contains("--startup"))
                        {
                            // Trigger migration in the background
                            System.Threading.Tasks.Task.Run(() => SetStartup(true));
                        }
                        return true;
                    }
                    return false;
                }
            }
            catch
            {
                return false;
            }
        }
    }
}
