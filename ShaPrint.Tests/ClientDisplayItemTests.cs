using System;
using Xunit;
using ShaPrint.Core.Network;

namespace ShaPrint.Tests
{
    public class ClientDisplayItemTests
    {
        // A minimal recreation of the WinForms PrinterDisplayItem logic to test formatting
        private class MockPrinterDisplayItem
        {
            public DiscoveryResponseMessage Server { get; set; } = new DiscoveryResponseMessage();
            public PrinterInfo Printer { get; set; } = new PrinterInfo();
            public bool IsInstalled { get; set; } = false;
            public bool IsVerified { get; set; } = false;
            
            public override string ToString() =>
                $"{(IsVerified ? "" : "[UNVERIFIED] ")}[{Server.ServerName}] {Printer.Name} {(IsInstalled ? "(INSTALLED)" : "")}".TrimEnd();
        }

        [Fact]
        public void PrinterDisplayItem_ShouldShowInstalledMarker_WhenInstalledIsTrue()
        {
            // Arrange
            var item = new MockPrinterDisplayItem
            {
                Server = new DiscoveryResponseMessage { ServerName = "SERVER-PC" },
                Printer = new PrinterInfo { Name = "Epson L120" },
                IsInstalled = true,
                IsVerified = true
            };

            // Act
            string display = item.ToString();

            // Assert
            Assert.Contains("(INSTALLED)", display);
            Assert.Equal("[SERVER-PC] Epson L120 (INSTALLED)", display);
        }

        [Fact]
        public void PrinterDisplayItem_ShouldNotShowInstalledMarker_WhenInstalledIsFalse()
        {
            // Arrange
            var item = new MockPrinterDisplayItem
            {
                Server = new DiscoveryResponseMessage { ServerName = "SERVER-PC" },
                Printer = new PrinterInfo { Name = "Epson L120" },
                IsInstalled = false,
                IsVerified = true
            };

            // Act
            string display = item.ToString();

            // Assert
            Assert.DoesNotContain("(INSTALLED)", display);
            Assert.Equal("[SERVER-PC] Epson L120", display);
        }
        
        [Fact]
        public void PrinterDisplayItem_ShouldShowUnverifiedMarker_WhenNotVerified()
        {
            // Arrange
            var item = new MockPrinterDisplayItem
            {
                Server = new DiscoveryResponseMessage { ServerName = "SERVER-PC" },
                Printer = new PrinterInfo { Name = "Epson L120" },
                IsInstalled = false,
                IsVerified = false
            };

            // Act
            string display = item.ToString();

            // Assert
            Assert.StartsWith("[UNVERIFIED]", display);
            Assert.Equal("[UNVERIFIED] [SERVER-PC] Epson L120", display);
        }
    }
}
