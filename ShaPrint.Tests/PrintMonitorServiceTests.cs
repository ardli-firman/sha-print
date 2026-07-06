using System.Printing;
using ShaPrint.WpfApp.Services.Server;
using Xunit;

namespace ShaPrint.Tests
{
    public class PrintMonitorServiceTests
    {
        [Theory]
        [InlineData(PrintJobStatus.Error, true)]
        [InlineData(PrintJobStatus.PaperOut, true)]
        [InlineData(PrintJobStatus.Blocked, true)]
        [InlineData(PrintJobStatus.Offline, true)]
        [InlineData(PrintJobStatus.Error | PrintJobStatus.Printing, true)]
        [InlineData(PrintJobStatus.PaperOut | PrintJobStatus.Retained, true)]
        public void IsJobInErrorState_ShouldReturnTrue_ForErrorStatuses(PrintJobStatus status, bool expected)
        {
            // Act
            var result = PrintMonitorService.IsJobInErrorState(status);

            // Assert
            Assert.Equal(expected, result);
        }

        [Theory]
        [InlineData(PrintJobStatus.None, false)]
        [InlineData(PrintJobStatus.Printing, false)]
        [InlineData(PrintJobStatus.Spooling, false)]
        [InlineData(PrintJobStatus.Retained, false)]
        [InlineData(PrintJobStatus.Printed, false)]
        [InlineData(PrintJobStatus.Deleted, false)]
        [InlineData(PrintJobStatus.Printing | PrintJobStatus.Spooling, false)]
        public void IsJobInErrorState_ShouldReturnFalse_ForNormalStatuses(PrintJobStatus status, bool expected)
        {
            // Act
            var result = PrintMonitorService.IsJobInErrorState(status);

            // Assert
            Assert.Equal(expected, result);
        }

        [Fact]
        public void SetMonitoredPrinters_ShouldUpdateInternalList()
        {
            // Arrange
            var service = new PrintMonitorService(null!, null!);
            var printers = new System.Collections.Generic.List<string> { "PrinterA", "PrinterB" };

            // Act
            service.SetMonitoredPrinters(printers);

            // Assert
            var field = typeof(PrintMonitorService).GetField("_monitoredPrinters", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
            var monitored = field!.GetValue(service) as System.Collections.Generic.List<string>;

            Assert.NotNull(monitored);
            Assert.Equal(2, monitored.Count);
            Assert.Contains("PrinterA", monitored);
            Assert.Contains("PrinterB", monitored);
        }

        [Fact]
        public void SetMonitoredPrinters_WithNull_ShouldInitializeEmptyList()
        {
            // Arrange
            var service = new PrintMonitorService(null!, null!);

            // Act
            service.SetMonitoredPrinters(null!);

            // Assert
            var field = typeof(PrintMonitorService).GetField("_monitoredPrinters", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
            var monitored = field!.GetValue(service) as System.Collections.Generic.List<string>;

            Assert.NotNull(monitored);
            Assert.Empty(monitored);
        }
    }
}
