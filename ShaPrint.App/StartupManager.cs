using System;
using System.Windows.Forms;
using Microsoft.Win32;

namespace ShaPrint.App
{
    public static class StartupManager
    {
        private const string AppName = "ShaPrint";

        public static void SetStartup(bool enable)
        {
            try
            {
                string exePath = Application.ExecutablePath;
                using (RegistryKey? key = Registry.CurrentUser.OpenSubKey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run", true))
                {
                    if (key != null)
                    {
                        if (enable)
                        {
                            key.SetValue(AppName, $"\"{exePath}\"");
                            ShaPrint.Core.AppLogger.Log("[SYSTEM] Enabled Run on Windows Startup.");
                        }
                        else
                        {
                            key.DeleteValue(AppName, false);
                            ShaPrint.Core.AppLogger.Log("[SYSTEM] Disabled Run on Windows Startup.");
                        }
                    }
                }
            }
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error("[SYSTEM] Failed to change startup settings: " + ex.Message);
            }
        }

        public static bool IsStartupEnabled()
        {
            try
            {
                using (RegistryKey? key = Registry.CurrentUser.OpenSubKey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run", false))
                {
                    return key?.GetValue(AppName) != null;
                }
            }
            catch
            {
                return false;
            }
        }
    }
}
