using System;
using System.Collections.Generic;
using System.Linq;
using Xunit;

namespace ShaPrint.Tests
{
    public class ClientPrinterDetectionTests
    {
        // A mock implementation of the logic from ClientViewModel.cs
        // to verify that old and new printer names are correctly detected.
        private bool CheckIsInstalled(string targetServerIp, string targetServerName, string targetPrinterName, 
            List<(string VirtualPrinterName, string TargetPrinterName, string ServerIp)> installedConfigs,
            List<string> localOsPrinters)
        {
            string virtualPrinterName = $"ShaPrint [{targetServerName}] - {targetPrinterName}";
            bool isInstalledConfig = installedConfigs.Any(p => p.VirtualPrinterName.Equals(virtualPrinterName, StringComparison.OrdinalIgnoreCase));
            bool isInstalledOs = localOsPrinters.Contains(virtualPrinterName, StringComparer.OrdinalIgnoreCase);

            // Fallback backward compatibility check for old format: "ShaPrint - {PrinterName}"
            if (!isInstalledConfig && !isInstalledOs)
            {
                isInstalledConfig = installedConfigs.Any(p => 
                    p.TargetPrinterName.Equals(targetPrinterName, StringComparison.OrdinalIgnoreCase) && 
                    p.ServerIp.Equals(targetServerIp));
                    
                string oldName = $"ShaPrint - {targetPrinterName}";
                isInstalledOs = localOsPrinters.Contains(oldName, StringComparer.OrdinalIgnoreCase);
            }

            return isInstalledConfig || isInstalledOs;
        }

        [Fact]
        public void Scan_ShouldDetectNewFormat_WhenConfigured()
        {
            var configs = new List<(string VirtualPrinterName, string TargetPrinterName, string ServerIp)>
            {
                ("ShaPrint [SERVER1] - MyPrinter", "MyPrinter", "192.168.1.100")
            };
            
            var osPrinters = new List<string> { "ShaPrint [SERVER1] - MyPrinter" };

            bool isInstalled = CheckIsInstalled("192.168.1.100", "SERVER1", "MyPrinter", configs, osPrinters);
            Assert.True(isInstalled);
        }

        [Fact]
        public void Scan_ShouldDetectOldFormat_FromConfig()
        {
            var configs = new List<(string VirtualPrinterName, string TargetPrinterName, string ServerIp)>
            {
                // Old config format didn't have ServerName in VirtualPrinterName
                ("ShaPrint - OldPrinter", "OldPrinter", "10.0.0.5")
            };
            
            var osPrinters = new List<string>();

            bool isInstalled = CheckIsInstalled("10.0.0.5", "SERVER_XYZ", "OldPrinter", configs, osPrinters);
            Assert.True(isInstalled);
        }

        [Fact]
        public void Scan_ShouldDetectOldFormat_FromOsPrinters()
        {
            var configs = new List<(string VirtualPrinterName, string TargetPrinterName, string ServerIp)>();
            
            // The printer exists in OS but config was lost/corrupted
            var osPrinters = new List<string> { "ShaPrint - LegacyPrinter" };

            bool isInstalled = CheckIsInstalled("127.0.0.1", "ANY_SERVER", "LegacyPrinter", configs, osPrinters);
            Assert.True(isInstalled);
        }

        [Fact]
        public void Scan_ShouldNotDetect_IfDifferentPrinter()
        {
            var configs = new List<(string VirtualPrinterName, string TargetPrinterName, string ServerIp)>
            {
                ("ShaPrint - PrinterA", "PrinterA", "10.0.0.1")
            };
            var osPrinters = new List<string>();

            // Scanning for PrinterB
            bool isInstalled = CheckIsInstalled("10.0.0.1", "SERVER1", "PrinterB", configs, osPrinters);
            Assert.False(isInstalled);
        }
    }
}
