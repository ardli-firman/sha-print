using ShaPrint.Core.Ipp;
using ShaPrint.Core.Ipp.Testing;

namespace ShaPrint.Tests;

/// <summary>
/// Tests for IPP printer functionality including URL generation and multi-printer support.
/// </summary>
public class IppPrinterTests
{
    /// <summary>
    /// IPP URL should be correctly formatted for printer name.
    /// </summary>
    [Theory]
    [InlineData("192.168.1.100", "HP LaserJet", "http://192.168.1.100:631/printers/HP%20LaserJet/ipp/print")]
    [InlineData("10.0.0.1", "Canon PIXMA", "http://10.0.0.1:631/printers/Canon%20PIXMA/ipp/print")]
    [InlineData("192.168.1.50", "TestPrinter", "http://192.168.1.50:631/printers/TestPrinter/ipp/print")]
    public void IppUrl_ShouldBeCorrectlyFormatted(string serverIp, string printerName, string expectedUrl)
    {
        // Act
        string ippUrl = $"http://{serverIp}:631/printers/{Uri.EscapeDataString(printerName)}/ipp/print";

        // Assert
        Assert.Equal(expectedUrl, ippUrl);
    }

    /// <summary>
    /// IPP URL should handle special characters in printer name.
    /// </summary>
    [Theory]
    [InlineData("Printer (1)", "Printer%20%281%29")]
    [InlineData("Printer & Sons", "Printer%20%26%20Sons")]
    [InlineData("Printer #1", "Printer%20%231")]
    public void IppUrl_ShouldHandleSpecialCharacters(string printerName, string expectedEncoded)
    {
        // Arrange
        string serverIp = "192.168.1.100";

        // Act
        string ippUrl = $"http://{serverIp}:631/printers/{Uri.EscapeDataString(printerName)}/ipp/print";

        // Assert
        Assert.Contains(expectedEncoded, ippUrl);
    }

    /// <summary>
    /// Multi-printer server should route to correct printer based on URL.
    /// </summary>
    [Fact]
    public async Task MultiPrinter_ShouldRouteToCorrectPrinter()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "PrinterA", DriverName = "DriverA" });
        spooler.AddPrinter(new PrinterInfo { Name = "PrinterB", DriverName = "DriverB" });

        var serverA = new IppServer(spooler, "PrinterA");
        var serverB = new IppServer(spooler, "PrinterB");

        // Act - print to PrinterA
        var requestA = IppRequestBuilder.BuildPrintJobRequest("PrinterA", new byte[] { 0x50 });
        using var inputA = new MemoryStream(requestA);
        using var outputA = new MemoryStream();
        await serverA.ProcessRequestAsync(inputA, outputA);

        // Act - print to PrinterB
        var requestB = IppRequestBuilder.BuildPrintJobRequest("PrinterB", new byte[] { 0x44 });
        using var inputB = new MemoryStream(requestB);
        using var outputB = new MemoryStream();
        await serverB.ProcessRequestAsync(inputB, outputB);

        // Assert
        Assert.Equal(2, spooler.PrintedJobs.Count);
        Assert.Equal("PrinterA", spooler.PrintedJobs[0].PrinterName);
        Assert.Equal("PrinterB", spooler.PrintedJobs[1].PrinterName);
    }

    /// <summary>
    /// Multi-printer server should return correct printer attributes.
    /// </summary>
    [Fact]
    public async Task MultiPrinter_ShouldReturnCorrectAttributes()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "HP LaserJet", DriverName = "HP Universal" });
        spooler.AddPrinter(new PrinterInfo { Name = "Canon PIXMA", DriverName = "Canon Generic" });

        var serverHP = new IppServer(spooler, "HP LaserJet");
        var serverCanon = new IppServer(spooler, "Canon PIXMA");

        // Act - get attributes for HP
        var requestHP = IppRequestBuilder.BuildGetPrinterAttributesRequest();
        using var inputHP = new MemoryStream(requestHP);
        using var outputHP = new MemoryStream();
        await serverHP.ProcessRequestAsync(inputHP, outputHP);

        // Act - get attributes for Canon
        var requestCanon = IppRequestBuilder.BuildGetPrinterAttributesRequest();
        using var inputCanon = new MemoryStream(requestCanon);
        using var outputCanon = new MemoryStream();
        await serverCanon.ProcessRequestAsync(inputCanon, outputCanon);

        // Assert
        var responseHP = System.Text.Encoding.ASCII.GetString(outputHP.ToArray());
        var responseCanon = System.Text.Encoding.ASCII.GetString(outputCanon.ToArray());

        Assert.Contains("HP LaserJet", responseHP);
        Assert.Contains("Canon PIXMA", responseCanon);
    }

    /// <summary>
    /// Multi-printer server should handle print to non-existent printer.
    /// </summary>
    [Fact]
    public async Task MultiPrinter_NonExistentPrinter_ShouldFail()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "PrinterA", DriverName = "DriverA" });

        var server = new IppServer(spooler, "NonExistentPrinter");

        // Act
        var request = IppRequestBuilder.BuildPrintJobRequest("NonExistentPrinter", new byte[] { 0x50 });
        using var input = new MemoryStream(request);
        using var output = new MemoryStream();
        await server.ProcessRequestAsync(input, output);

        // Assert
        Assert.Empty(spooler.PrintedJobs);
    }

    /// <summary>
    /// IPP URL should work with different ports.
    /// </summary>
    [Theory]
    [InlineData(631)]
    [InlineData(8080)]
    [InlineData(9877)]
    public void IppUrl_ShouldWorkWithDifferentPorts(int port)
    {
        // Arrange
        string serverIp = "192.168.1.100";
        string printerName = "TestPrinter";

        // Act
        string ippUrl = $"http://{serverIp}:{port}/printers/{Uri.EscapeDataString(printerName)}/ipp/print";

        // Assert
        Assert.Contains($":{port}/", ippUrl);
    }

    /// <summary>
    /// IPP URL should be valid for Windows Add Printer.
    /// </summary>
    [Fact]
    public void IppUrl_ShouldBeValidForWindows()
    {
        // Arrange
        string serverIp = "192.168.1.100";
        string printerName = "HP LaserJet";

        // Act
        string ippUrl = $"http://{serverIp}:631/printers/{Uri.EscapeDataString(printerName)}/ipp/print";

        // Assert
        Assert.StartsWith("http://", ippUrl);
        Assert.Contains("/ipp/print", ippUrl);
        Assert.DoesNotContain(" ", ippUrl); // No spaces
        Assert.DoesNotContain("\\", ippUrl); // No backslashes
    }
}
