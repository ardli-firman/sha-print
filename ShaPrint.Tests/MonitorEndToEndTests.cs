using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.ViewModels.Pages;
using ShaPrint.WpfApp.Services.Monitor;
using ShaPrint.WpfApp.Services;
using ShaPrint.WpfApp.Services.Server;
using ShaPrint.Server;
using Xunit;

namespace ShaPrint.Tests
{
    [Collection("SequentialNetworkTests")]
    public class MonitorEndToEndTests
    {
        private class DummyNotificationService : INotificationService
        {
            public void ShowClientConnected(string clientIp) { }
            public void ShowClientDisconnected(string clientIp) { }
            public void ShowPrintJobCompleted(string documentName, string printerName) { }
            public void ShowPrintJobFailed(string documentName, string printerName, string reason) { }
            public void ShowScanCompleted(string fileName) { }
            public void ShowScanFailed(string errorMessage) { }
            public void ShowPrinterError(string printerName, string errorDescription) { }
            public void ShowSecurityAlert(string message, string detail) { }
            public void ShowToast(string title, string body, ToastAction? action = null) { }
        }

        [Fact]
        public async Task Test_Monitor_EndToEnd_Flow_Success()
        {
            var dummyNotification = new DummyNotificationService();

            // 1. Arrange: Setup and start Server Mode
            using var serverVm = new ServerViewModel(null!, null!, null!, dummyNotification);
            
            // Expose a printer and scanner for the status payload
            serverVm.ExposedPrinters.Add("Office-LaserJet");
            serverVm.ExposedScanners.Add("Office-Flatbed");

            // Start Server
            serverVm.GetType().GetMethod("StartServer", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance)!.Invoke(serverVm, null);

            // Wait a brief moment for socket listeners to bind
            await Task.Delay(500);

            // 2. Arrange: Setup and start Monitor Mode Client
            var monitorVm = new MonitorViewModel();
            var monitorService = new MonitorService(monitorVm);

            try
            {
                // Act: Trigger Manual Refresh (does discovery + TCP status poll)
                await monitorService.TriggerManualRefreshAsync();

                // Wait for background tasks to complete (up to 3 seconds)
                int retries = 30;
                while (retries > 0 && (monitorVm.Servers.Count == 0 || monitorVm.Servers[0].Payload == null))
                {
                    await Task.Delay(100);
                    retries--;
                }

                // Assert
                Assert.NotEmpty(monitorVm.Servers);
                var node = monitorVm.Servers.FirstOrDefault(s => s.HostName.Equals(Environment.MachineName, StringComparison.OrdinalIgnoreCase));
                Assert.NotNull(node);
                Assert.Contains(node!.Status, new[] { "Online", "Warning" });
                Assert.False(string.IsNullOrEmpty(node.IpAddress));
                
                // Assert payload values
                Assert.NotNull(node.Payload);
                Assert.Equal(Environment.MachineName, node.Payload!.ServerName);
                Assert.Single(node.Payload.Printers);
                Assert.Equal("Office-LaserJet", node.Payload.Printers[0].Name);
                Assert.Single(node.Payload.Scanners);
                Assert.Equal("Office-Flatbed", node.Payload.Scanners[0].Name);
            }
            finally
            {
                // Clean up: Stop Server and Monitor
                monitorService.Stop();
                serverVm.StopServer();
            }
        }
    }
}
