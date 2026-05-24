using ShaPrint.Core;
using System;
using Xunit;

namespace ShaPrint.Tests;

public class ValidatorsTests
{
    // ── ValidatePrinterName ──────────────────────────

    [Theory]
    [InlineData("Epson L3210 Series")]
    [InlineData("HP LaserJet Pro M404dn")]
    [InlineData("Generic / Text Only")]
    [InlineData("PDF995")]
    [InlineData("Microsoft XPS Document Writer")]
    [InlineData("PRINTER-01")]
    [InlineData("Printer_Name_123")]
    [InlineData("Foo (Copy 1)")]
    [InlineData("a")] // minimum
    public void ValidatePrinterName_Valid_ReturnsTrimmed(string name)
    {
        string result = Validators.ValidatePrinterName(name);
        Assert.Equal(name.Trim(), result);
    }

    [Theory]
    [InlineData("Epson'; calc; #")]
    [InlineData("\"injected\"")]
    [InlineData("printer`name")]
    [InlineData("printer$name")]
    [InlineData("printer|name")]
    [InlineData("printer&name")]
    [InlineData("printer<name")]
    [InlineData("printer>name")]
    [InlineData("printer\nname")]
    [InlineData("printer\rname")]
    [InlineData("printer\tname")]
    [InlineData("printer\0name")]
    [InlineData("'; Start-Process malware.exe; #")]
    public void ValidatePrinterName_DangerousChars_Throws(string name)
    {
        Assert.Throws<ArgumentException>(() => Validators.ValidatePrinterName(name));
    }

    [Fact]
    public void ValidatePrinterName_Empty_Throws()
    {
        Assert.Throws<ArgumentException>(() => Validators.ValidatePrinterName(""));
        Assert.Throws<ArgumentException>(() => Validators.ValidatePrinterName("   "));
        Assert.Throws<ArgumentException>(() => Validators.ValidatePrinterName(null!));
    }

    [Fact]
    public void ValidatePrinterName_TooLong_Throws()
    {
        string longName = new string('A', Validators.MaxPrinterNameLength + 1);
        Assert.Throws<ArgumentException>(() => Validators.ValidatePrinterName(longName));
    }

    [Fact]
    public void ValidatePrinterName_MaxLength_Succeeds()
    {
        string maxName = new string('A', Validators.MaxPrinterNameLength);
        string result = Validators.ValidatePrinterName(maxName);
        Assert.Equal(maxName, result);
    }

    // ── ValidateServerName ───────────────────────────

    [Theory]
    [InlineData("SRV-WORKSTATION")]
    [InlineData("DESKTOP-ABC123")]
    [InlineData("MyPC")]
    [InlineData("server01")]
    public void ValidateServerName_Valid_ReturnsTrimmed(string name)
    {
        string result = Validators.ValidateServerName(name);
        Assert.Equal(name.Trim(), result);
    }

    [Theory]
    [InlineData("SRV'; Invoke-Command")]
    [InlineData("server\"name\"")]
    [InlineData("bad`server")]
    public void ValidateServerName_DangerousChars_Throws(string name)
    {
        Assert.Throws<ArgumentException>(() => Validators.ValidateServerName(name));
    }

    [Fact]
    public void ValidateServerName_Empty_Throws()
    {
        Assert.Throws<ArgumentException>(() => Validators.ValidateServerName(""));
        Assert.Throws<ArgumentException>(() => Validators.ValidateServerName(null!));
    }

    // ── ValidateDriverName ───────────────────────────

    [Theory]
    [InlineData("Epson L3210 Series")]
    [InlineData("Generic / Text Only")]
    [InlineData("HP Universal Printing PCL 6")]
    public void ValidateDriverName_Valid_ReturnsTrimmed(string name)
    {
        string result = Validators.ValidateDriverName(name);
        Assert.Equal(name.Trim(), result);
    }

    [Theory]
    [InlineData("Epson'; evil")]
    [InlineData("driver\"quote")]
    public void ValidateDriverName_DangerousChars_Throws(string name)
    {
        Assert.Throws<ArgumentException>(() => Validators.ValidateDriverName(name));
    }

    [Fact]
    public void ValidateDriverName_Empty_Throws()
    {
        Assert.Throws<ArgumentException>(() => Validators.ValidateDriverName(""));
        Assert.Throws<ArgumentException>(() => Validators.ValidateDriverName(null!));
    }

    // ── ValidateIpAddress ────────────────────────────

    [Theory]
    [InlineData("192.168.1.1")]
    [InlineData("10.0.0.1")]
    [InlineData("172.16.0.1")]
    [InlineData("127.0.0.1")]
    [InlineData("0.0.0.0")]
    [InlineData("255.255.255.255")]
    public void ValidateIpAddress_Valid_ReturnsTrimmed(string ip)
    {
        string result = Validators.ValidateIpAddress(ip);
        Assert.Equal(ip.Trim(), result);
    }

    [Theory]
    [InlineData("not-an-ip")]
    [InlineData("999.999.999.999")]
    [InlineData("256.1.1.1")]
    [InlineData("192.168.1.1.1")]
    [InlineData("192.168.1.; DROP TABLE")]
    [InlineData("")]
    public void ValidateIpAddress_Invalid_Throws(string ip)
    {
        Assert.Throws<ArgumentException>(() => Validators.ValidateIpAddress(ip));
    }
}
