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

        /// <summary>
        /// Checks if firewall rules exist. If they do, logs and returns silently (no elevation needed).
        /// If they are missing, prompts the user once to add them. Rules are permanent — they are NOT
        /// removed when the server stops, so subsequent starts and auto-start boots will pass the check
        /// silently without any UAC prompt.
        /// </summary>
        public static void EnsureFirewallRules()
        {
            if (_isCheckingFirewall) return;
            _isCheckingFirewall = true;

            Task.Run(() =>
            {
                try
                {
                    bool tcpExists = CheckRuleExists("ShaPrint Server TCP");
                    bool udpExists = CheckRuleExists("ShaPrint Server UDP");

                    if (tcpExists && udpExists)
                    {
                        AppLogger.Log("[SERVER] Firewall rules verified — ports are open.");
                        return;
                    }

                    // Rules are missing — prompt user (first-time setup only)
                    MessageBoxResult result = MessageBoxResult.No;
                    try
                    {
                        result = MessageBox.Show(
                            "ShaPrint Server needs to open network ports (TCP 9877 & UDP 9876) in Windows Firewall.\n\n" +
                            "This is a one-time setup. After the rules are added, you will NOT be prompted again on future starts.\n\n" +
                            "Configure now?", 
                            "Firewall Configuration (one-time)", 
                            MessageBoxButton.YesNo, 
                            MessageBoxImage.Question);
                    }
                    catch (Exception ex)
                    {
                        AppLogger.Error("Failed to show firewall configuration prompt", ex);
                    }

                    if (result == MessageBoxResult.Yes)
                    {
                        if (!tcpExists) AddRule("ShaPrint Server TCP", "TCP", Constants.PrintTcpPort);
                        if (!udpExists) AddRule("ShaPrint Server UDP", "UDP", Constants.DiscoveryUdpPort);
                        AppLogger.Log("[SERVER] Firewall rules added successfully (persistent).");
                    }
                    else
                    {
                        AppLogger.Log("[SERVER] Firewall rules setup declined by user. Clients may not be able to connect.");
                    }
                }
                catch (Exception ex)
                {
                    AppLogger.Error("Firewall config error: " + ex.Message);
                }
                finally
                {
                    _isCheckingFirewall = false;
                }
            });
        }

        /// <summary>
        /// Legacy compatibility: old name still works as an alias.
        /// </summary>
        public static void CheckAndAddFirewallRules() => EnsureFirewallRules();

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
    }
}
