using System;
using System.Diagnostics;
using System.Windows;
using System.Threading.Tasks;
using ShaPrint.Core;

namespace ShaPrint.Server
{
    public static class FirewallManager
    {
        private static bool _isCheckingFirewall = false;

        public static void CheckAndAddFirewallRules()
        {
            if (_isCheckingFirewall) return;
            _isCheckingFirewall = true;

            Task.Run(() =>
            {
                try
                {
                    bool tcpExists = CheckRuleExists("ShaPrint Server TCP");
                    bool udpExists = CheckRuleExists("ShaPrint Server UDP");

                    if (!tcpExists || !udpExists)
                    {
                        MessageBoxResult result = MessageBoxResult.No;
                        try
                        {
                            result = MessageBox.Show(
                                "ShaPrint Server needs to open network ports (TCP 9877 & UDP 9876) in Windows Firewall to allow clients to connect.\n\nDo you want to configure this automatically now?", 
                                "Firewall Configuration", 
                                MessageBoxButton.YesNo, 
                                MessageBoxImage.Question);
                        }
                        catch (Exception ex)
                        {
                            ShaPrint.Core.AppLogger.Error("Failed to show firewall configuration prompt", ex);
                        }
                        if (result == MessageBoxResult.Yes)
                        {
                            if (!tcpExists) AddRule("ShaPrint Server TCP", "TCP", Constants.PrintTcpPort);
                            if (!udpExists) AddRule("ShaPrint Server UDP", "UDP", Constants.DiscoveryUdpPort);
                            AppLogger.Log("[SERVER] Firewall rules added successfully.");
                        }
                        else
                        {
                            AppLogger.Log("[SERVER] Firewall rules setup declined by user.");
                        }
                    }
                    else
                    {
                        AppLogger.Log("[SERVER] Firewall rules are active and verified.");
                    }
                }
                catch (Exception ex)
                {
                    ShaPrint.Core.AppLogger.Error("Firewall config error: " + ex.Message);
                }
                finally
                {
                    _isCheckingFirewall = false;
                }
            });
        }

        /// <summary>
        /// Removes both ShaPrint firewall rules synchronously. Called when server stops.
        /// Synchronous to ensure cleanup completes before process exit.
        /// </summary>
        public static void RemoveFirewallRules()
        {
            try
            {
                RemoveRule("ShaPrint Server TCP");
                RemoveRule("ShaPrint Server UDP");
                AppLogger.Log("[SERVER] Removed ShaPrint firewall rules.");
            }
            catch (Exception ex)
            {
                AppLogger.Error("[SERVER] Failed to remove firewall rules: " + ex.Message);
            }
        }

        private static bool CheckRuleExists(string ruleName)
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

        private static void AddRule(string ruleName, string protocol, int port)
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
            catch (System.ComponentModel.Win32Exception)
            {
                try
                {
                    MessageBox.Show($"Failed to add Firewall rule for {protocol} {port}. You may need to do this manually.", "UAC Cancelled", MessageBoxButton.OK, MessageBoxImage.Warning);
                }
                catch (Exception ex)
                {
                    AppLogger.Error("Failed to show UAC warning message", ex);
                }
            }
        }

        private static void RemoveRule(string ruleName)
        {
            var psi = new ProcessStartInfo
            {
                FileName = "netsh",
                Arguments = $"advfirewall firewall delete rule name=\"{ruleName}\"",
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
            catch (System.ComponentModel.Win32Exception)
            {
                // User declined UAC prompt — rule stays until manual cleanup
                AppLogger.Log($"[SERVER] Firewall: UAC declined for removing rule '{ruleName}'. Rule may persist.");
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[SERVER] Firewall: netsh delete failed for '{ruleName}': {ex.Message}");
            }
        }
    }
}
