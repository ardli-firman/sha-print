using System;
using System.Diagnostics;
using System.Threading.Tasks;
using System.Windows.Forms;
using ShaPrint.Core;

namespace ShaPrint.Server
{
    public static class FirewallManager
    {
        public static void CheckAndAddFirewallRules()
        {
            Task.Run(() =>
            {
                try
                {
                    bool tcpExists = CheckRuleExists("ShaPrint Server TCP");
                    bool udpExists = CheckRuleExists("ShaPrint Server UDP");

                    if (!tcpExists || !udpExists)
                    {
                        var result = MessageBox.Show(
                            "ShaPrint Server needs to open network ports (TCP 9877 & UDP 9876) in Windows Firewall to allow clients to connect.\n\nDo you want to configure this automatically now?", 
                            "Firewall Configuration", 
                            MessageBoxButtons.YesNo, 
                            MessageBoxIcon.Question);

                        if (result == DialogResult.Yes)
                        {
                            if (!tcpExists) AddRule("ShaPrint Server TCP", "TCP", Constants.PrintTcpPort);
                            if (!udpExists) AddRule("ShaPrint Server UDP", "UDP", Constants.DiscoveryUdpPort);
                        }
                    }
                }
                catch (Exception ex)
                {
                    Console.WriteLine("Firewall config error: " + ex.Message);
                }
            });
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
            
            // If the rule does not exist, netsh returns "No rules match the specified criteria."
            return process.ExitCode == 0 && !output.Contains("No rules match");
        }

        private static void AddRule(string ruleName, string protocol, int port)
        {
            // Requires Administrator privileges
            var psi = new ProcessStartInfo
            {
                FileName = "netsh",
                Arguments = $"advfirewall firewall add rule name=\"{ruleName}\" dir=in action=allow protocol={protocol} localport={port} profile=any",
                UseShellExecute = true, // Must be true for Verb = "runas"
                Verb = "runas",         // Request UAC elevation
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
                // User clicked "No" on the UAC prompt
                MessageBox.Show($"Failed to add Firewall rule for {protocol} {port}. You may need to do this manually.", "UAC Cancelled", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }
    }
}
