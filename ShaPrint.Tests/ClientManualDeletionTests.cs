using System;
using System.Collections.Generic;
using System.Linq;
using Xunit;

namespace ShaPrint.Tests
{
    public class ClientManualDeletionTests
    {
        // Helper mock structure simulating client configuration
        public class MockInstalledPrinterConfig
        {
            public string VirtualPrinterName { get; set; } = string.Empty;
            public string PipeName { get; set; } = string.Empty;
            public string ServerIp { get; set; } = string.Empty;
            public string TargetPrinterName { get; set; } = string.Empty;
        }

        // Mock implementation of LoadConfiguration's auto-clean logic
        private (List<MockInstalledPrinterConfig> ValidPrinters, bool ConfigChanged) SyncConfigWithOsPrinters(
            List<MockInstalledPrinterConfig> saved,
            List<string> localPrinters)
        {
            bool isSpoolerHealthy = localPrinters != null && localPrinters.Count > 0;
            var validPrinters = new List<MockInstalledPrinterConfig>();
            bool configChanged = false;

            foreach (var config in saved)
            {
                if (string.IsNullOrEmpty(config.PipeName) || string.IsNullOrEmpty(config.ServerIp))
                {
                    configChanged = true;
                    continue;
                }

                // Auto-clean config if the printer was manually deleted from Windows
                if (isSpoolerHealthy && !localPrinters.Contains(config.VirtualPrinterName, StringComparer.OrdinalIgnoreCase))
                {
                    string oldName = $"ShaPrint - {config.TargetPrinterName}";
                    if (!localPrinters.Contains(oldName, StringComparer.OrdinalIgnoreCase))
                    {
                        configChanged = true;
                        continue;
                    }
                }

                validPrinters.Add(config);
            }

            return (validPrinters, configChanged);
        }

        // Mock implementation of DeleteSelectedAsync success condition check
        private bool EvaluateDeleteSuccess(
            bool removePrinterSuccess, 
            string virtualPrinterName, 
            string targetPrinterName, 
            List<string> currentLocalPrinters)
        {
            bool alreadyDeletedInWindows = !currentLocalPrinters.Contains(virtualPrinterName, StringComparer.OrdinalIgnoreCase);
            if (alreadyDeletedInWindows)
            {
                string oldName = $"ShaPrint - {targetPrinterName}";
                if (currentLocalPrinters.Contains(oldName, StringComparer.OrdinalIgnoreCase))
                {
                    alreadyDeletedInWindows = false;
                }
            }

            return removePrinterSuccess || alreadyDeletedInWindows;
        }

        [Fact]
        public void LoadConfiguration_ShouldKeepPrinter_WhenItExistsInOS()
        {
            // Arrange
            var saved = new List<MockInstalledPrinterConfig>
            {
                new MockInstalledPrinterConfig
                {
                    VirtualPrinterName = "ShaPrint [SERVER1] - Epson L3210",
                    PipeName = "shaprint_pipe_1",
                    ServerIp = "192.168.1.5",
                    TargetPrinterName = "Epson L3210"
                }
            };
            var localPrinters = new List<string> { "ShaPrint [SERVER1] - Epson L3210" };

            // Act
            var (validPrinters, configChanged) = SyncConfigWithOsPrinters(saved, localPrinters);

            // Assert
            Assert.Single(validPrinters);
            Assert.False(configChanged);
        }

        [Fact]
        public void LoadConfiguration_ShouldPrunePrinter_WhenItIsManuallyDeletedFromWindows()
        {
            // Arrange
            var saved = new List<MockInstalledPrinterConfig>
            {
                new MockInstalledPrinterConfig
                {
                    VirtualPrinterName = "ShaPrint [SERVER1] - Epson L3210",
                    PipeName = "shaprint_pipe_1",
                    ServerIp = "192.168.1.5",
                    TargetPrinterName = "Epson L3210"
                }
            };
            // Windows has no printers installed
            var localPrinters = new List<string> { "Microsoft Print to PDF" };

            // Act
            var (validPrinters, configChanged) = SyncConfigWithOsPrinters(saved, localPrinters);

            // Assert
            Assert.Empty(validPrinters);
            Assert.True(configChanged);
        }

        [Fact]
        public void LoadConfiguration_ShouldNotPrunePrinter_WhenSpoolerIsStopped()
        {
            // Arrange
            var saved = new List<MockInstalledPrinterConfig>
            {
                new MockInstalledPrinterConfig
                {
                    VirtualPrinterName = "ShaPrint [SERVER1] - Epson L3210",
                    PipeName = "shaprint_pipe_1",
                    ServerIp = "192.168.1.5",
                    TargetPrinterName = "Epson L3210"
                }
            };
            // Spooler is down, returning an empty list
            var localPrinters = new List<string>();

            // Act
            var (validPrinters, configChanged) = SyncConfigWithOsPrinters(saved, localPrinters);

            // Assert
            // It should NOT prune because isSpoolerHealthy is false (safety check to avoid clearing config when Spooler is temporarily down)
            Assert.Single(validPrinters);
            Assert.False(configChanged);
        }

        [Fact]
        public void DeletePrinter_ShouldSucceed_WhenRemovePrinterApiSucceeds()
        {
            // Arrange
            string virtualPrinterName = "ShaPrint [SERVER1] - Epson L3210";
            string targetPrinterName = "Epson L3210";
            bool removePrinterSuccess = true;
            var localPrinters = new List<string> { "ShaPrint [SERVER1] - Epson L3210" };

            // Act
            bool deleteSucceeded = EvaluateDeleteSuccess(removePrinterSuccess, virtualPrinterName, targetPrinterName, localPrinters);

            // Assert
            Assert.True(deleteSucceeded);
        }

        [Fact]
        public void DeletePrinter_ShouldSucceed_WhenPrinterIsAlreadyManuallyDeletedInOS()
        {
            // Arrange
            string virtualPrinterName = "ShaPrint [SERVER1] - Epson L3210";
            string targetPrinterName = "Epson L3210";
            // Windows API returns failure because it has already been manually deleted
            bool removePrinterSuccess = false; 
            var localPrinters = new List<string> { "Microsoft Print to PDF" }; // Already deleted from OS

            // Act
            bool deleteSucceeded = EvaluateDeleteSuccess(removePrinterSuccess, virtualPrinterName, targetPrinterName, localPrinters);

            // Assert
            // Should succeed because alreadyDeletedInWindows is true
            Assert.True(deleteSucceeded);
        }
    }
}
