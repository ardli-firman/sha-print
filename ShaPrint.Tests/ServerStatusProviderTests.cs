using System;
using System.Collections.Generic;
using System.Linq;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.ViewModels.Pages;
using ShaPrint.WpfApp.Services.Server;
using ShaPrint.WpfApp.Services;
using Xunit;

namespace ShaPrint.Tests
{
    public class ServerStatusProviderTests
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
        public void BuildStatus_PopulatesAllFieldsCorrectly()
        {
            // Arrange
            var dummyNotification = new DummyNotificationService();
            var vm = new ServerViewModel(null!, null!, null!, dummyNotification);
            
            // Set some exposed devices
            vm.ExposedPrinters.Add("Test Printer 1");
            vm.ExposedScanners.Add("Test Scanner 1");

            // Seed history
            vm.LogJob(new JobHistoryEntry
            {
                Type = "print",
                Document = "test.pdf",
                PrinterName = "Test Printer 1",
                ClientIp = "192.168.1.50",
                Status = "completed",
                Timestamp = DateTime.UtcNow
            });

            vm.LogError(new ServerErrorEntry
            {
                Source = "Test",
                Message = "Simulated error",
                Timestamp = DateTime.UtcNow
            });

            var provider = new ServerStatusProvider(vm);

            // Act
            var payload = provider.BuildStatus();

            // Assert
            Assert.NotNull(payload);
            Assert.Equal(Environment.MachineName, payload.ServerName);
            Assert.Equal(Environment.MachineName, payload.HostName);
            Assert.Single(payload.Printers);
            Assert.Equal("Test Printer 1", payload.Printers[0].Name);
            Assert.Single(payload.Scanners);
            Assert.Equal("Test Scanner 1", payload.Scanners[0].Name);
            Assert.Single(payload.RecentJobs);
            Assert.Equal("test.pdf", payload.RecentJobs[0].Document);
            Assert.Single(payload.Errors);
            Assert.Equal("Simulated error", payload.Errors[0].Message);
        }

        [Fact]
        public void HistoryQueues_EnforceCapacityLimits()
        {
            // Arrange
            var dummyNotification = new DummyNotificationService();
            var vm = new ServerViewModel(null!, null!, null!, dummyNotification);

            // Act: Enqueue 100 jobs
            for (int i = 1; i <= 100; i++)
            {
                vm.LogJob(new JobHistoryEntry
                {
                    Type = "print",
                    Document = $"job_{i}.pdf",
                    PrinterName = "Printer",
                    ClientIp = "127.0.0.1",
                    Status = "completed",
                    Timestamp = DateTime.UtcNow
                });
            }

            // Act: Enqueue 100 errors
            for (int i = 1; i <= 100; i++)
            {
                vm.LogError(new ServerErrorEntry
                {
                    Source = "Test",
                    Message = $"error_{i}",
                    Timestamp = DateTime.UtcNow
                });
            }

            // Assert: capacity capped at 50, and contains the LATEST 50 items
            Assert.Equal(50, vm.RecentJobs.Count);
            Assert.Equal("job_51.pdf", vm.RecentJobs.First().Document);
            Assert.Equal("job_100.pdf", vm.RecentJobs.Last().Document);

            Assert.Equal(50, vm.Errors.Count);
            Assert.Equal("error_51", vm.Errors.First().Message);
            Assert.Equal("error_100", vm.Errors.Last().Message);
        }
    }
}
