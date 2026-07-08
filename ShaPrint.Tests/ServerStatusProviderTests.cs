using System;
using System.Collections.Generic;
using System.Linq;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.Models;
using ShaPrint.WpfApp.Services.Server;
using ShaPrint.WpfApp.ViewModels.Pages;
using Xunit;

namespace ShaPrint.Tests
{
    public class ServerStatusProviderTests
    {
        private static ServerViewModel CreateSvm(DateTime? startTime = null)
        {
            var svm = new ServerViewModel(null!, null!, null!, null!);
            if (startTime.HasValue)
            {
                var property = typeof(ServerViewModel).GetProperty("ServerStartTime",
                    System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Instance);
                property!.SetValue(svm, startTime.Value);
            }
            return svm;
        }

        [Fact]
        public void BuildStatus_ServerName_HostName_AreMachineName()
        {
            // Arrange
            var svm = CreateSvm();
            var provider = new ServerStatusProvider(svm);

            // Act
            var payload = provider.BuildStatus();

            // Assert
            Assert.Equal(Environment.MachineName, payload.ServerName);
            Assert.Equal(Environment.MachineName, payload.HostName);
            Assert.Equal(AppSettings.Current.NetworkChannel, payload.NetworkChannel);
        }

        [Fact]
        public void BuildStatus_Uptime_WhenServerNotStarted_ReturnsZero()
        {
            // Arrange — no ServerStartTime set (null)
            var svm = CreateSvm(null);
            var provider = new ServerStatusProvider(svm);

            // Act
            var payload = provider.BuildStatus();

            // Assert
            Assert.Equal(0, payload.UptimeSeconds);
        }

        [Fact]
        public void BuildStatus_Uptime_WhenServerStarted_ReturnsPositive()
        {
            // Arrange
            var startTime = DateTime.UtcNow.AddHours(-2);
            var svm = CreateSvm(startTime);
            var provider = new ServerStatusProvider(svm);

            // Act
            var payload = provider.BuildStatus();

            // Assert
            Assert.True(payload.UptimeSeconds >= 7190); // ~2 hours minus small delta
            Assert.True(payload.UptimeSeconds <= 7220);
        }

        [Fact]
        public void BuildStatus_NoExposedPrinters_ReturnsEmptyPrinters()
        {
            // Arrange
            var svm = CreateSvm(DateTime.UtcNow);
            // ExposedPrinters is empty by default
            var provider = new ServerStatusProvider(svm);

            // Act
            var payload = provider.BuildStatus();

            // Assert
            Assert.NotNull(payload.Printers);
            Assert.Empty(payload.Printers);
        }

        [Fact]
        public void BuildStatus_NoExposedScanners_ReturnsEmptyScanners()
        {
            // Arrange
            var svm = CreateSvm(DateTime.UtcNow);
            var provider = new ServerStatusProvider(svm);

            // Act
            var payload = provider.BuildStatus();

            // Assert
            Assert.NotNull(payload.Scanners);
            Assert.Empty(payload.Scanners);
        }

        [Fact]
        public void BuildStatus_ActiveClients_ReturnsEmptyWhenNoDiscoveryServer()
        {
            // Arrange — DiscoveryServer is null by default
            var svm = CreateSvm(DateTime.UtcNow);
            var provider = new ServerStatusProvider(svm);

            // Act
            var payload = provider.BuildStatus();

            // Assert — should not throw
            Assert.NotNull(payload.ActiveClients);
        }

        [Fact]
        public void BuildStatus_Version_ReturnsAssemblyVersion()
        {
            // Arrange
            var svm = CreateSvm(DateTime.UtcNow);
            var provider = new ServerStatusProvider(svm);

            // Act
            var payload = provider.BuildStatus();

            // Assert
            Assert.False(string.IsNullOrEmpty(payload.Version));
            Assert.Matches(@"^\d+\.\d+\.\d+\.\d+$", payload.Version);
        }

        [Fact]
        public void BuildStatus_RecentJobs_FromViewModel()
        {
            // Arrange
            var svm = CreateSvm(DateTime.UtcNow);
            svm.RecentJobs.Enqueue(new JobHistoryEntry
            {
                Type = "print",
                Document = "test.doc",
                PrinterName = "P1",
                ClientIp = "10.0.0.1",
                Status = "completed",
                Timestamp = DateTime.UtcNow
            });

            var provider = new ServerStatusProvider(svm);

            // Act
            var payload = provider.BuildStatus();

            // Assert
            var job = Assert.Single(payload.RecentJobs);
            Assert.Equal("print", job.Type);
            Assert.Equal("P1", job.PrinterName);
        }

        [Fact]
        public void BuildStatus_Errors_FromViewModel()
        {
            // Arrange
            var svm = CreateSvm(DateTime.UtcNow);
            svm.Errors.Enqueue(new ServerErrorEntry
            {
                Source = "PrintMonitor",
                Message = "Test error",
                Timestamp = DateTime.UtcNow
            });

            var provider = new ServerStatusProvider(svm);

            // Act
            var payload = provider.BuildStatus();

            // Assert
            var err = Assert.Single(payload.Errors);
            Assert.Equal("PrintMonitor", err.Source);
            Assert.Equal("Test error", err.Message);
        }
    }
}
