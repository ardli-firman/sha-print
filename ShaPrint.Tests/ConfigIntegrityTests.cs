using ShaPrint.Core;
using System;
using System.Text.Json;
using Xunit;

namespace ShaPrint.Tests;

public class ConfigIntegrityTests
{
    // ── Server config ─────────────────────────────────

    [Fact]
    public void ServerConfig_RoundTrip_PreservesData()
    {
        var printers = new[] { "Epson L3210 Series", "HP LaserJet Pro", "PDF995" };
        string json = JsonSerializer.Serialize(printers);

        string wrapped = CryptoHelper.WrapConfigWithHmac(json);
        Assert.Contains("<!--HMAC:", wrapped);

        string? unwrapped = CryptoHelper.UnwrapConfigWithHmac(wrapped);
        Assert.NotNull(unwrapped);

        var recovered = JsonSerializer.Deserialize<string[]>(unwrapped!);
        Assert.NotNull(recovered);
        Assert.Equal(printers, recovered);
    }

    [Fact]
    public void ServerConfig_TamperedPrinterList_ReturnsNull()
    {
        var printers = new[] { "Epson L3210" };
        string json = JsonSerializer.Serialize(printers);
        string wrapped = CryptoHelper.WrapConfigWithHmac(json);

        // Attacker changes printer list
        string tampered = wrapped.Replace("Epson L3210", "Epson L3210'; evil; #");

        Assert.Null(CryptoHelper.UnwrapConfigWithHmac(tampered));
    }

    // ── Client config ─────────────────────────────────

    [Fact]
    public void ClientConfig_RoundTrip_PreservesData()
    {
        var configs = new[]
        {
            new { VirtualPrinterName = "ShaPrint - EPSON", PipeName = @"\\.\pipe\shaprint_abc123", ServerIp = "192.168.1.100", TargetPrinterName = "EPSON" }
        };
        string json = JsonSerializer.Serialize(configs);

        string wrapped = CryptoHelper.WrapConfigWithHmac(json);
        Assert.NotNull(CryptoHelper.UnwrapConfigWithHmac(wrapped));

        var recovered = JsonSerializer.Deserialize<dynamic[]>(CryptoHelper.UnwrapConfigWithHmac(wrapped)!);
        Assert.NotNull(recovered);
        Assert.Single(recovered!);
    }

    // ── Legacy config (no HMAC) ───────────────────────

    [Fact]
    public void LegacyConfig_PlainJson_UnwrapReturnsNull()
    {
        string plain = "{\"plain\":\"json\"}";
        Assert.Null(CryptoHelper.UnwrapConfigWithHmac(plain));
        // Caller should fall back to raw JSON for legacy support
    }

    // ── Malformed wrappers ────────────────────────────

    [Theory]
    [InlineData("plain text")]
    [InlineData("<!--HMAC:incomplete")]
    [InlineData("some\n<!--HMAC:ABC-->trailing")]
    public void UnwrapConfigWithHmac_Malformed_ReturnsNull(string input)
    {
        Assert.Null(CryptoHelper.UnwrapConfigWithHmac(input));
    }

    // ── Deterministic wrapping ────────────────────────

    [Fact]
    public void WrapConfigWithHmac_SameInput_SameOutput()
    {
        string json = "{\"fixed\":\"data\"}";
        string w1 = CryptoHelper.WrapConfigWithHmac(json);
        string w2 = CryptoHelper.WrapConfigWithHmac(json);
        Assert.Equal(w1, w2);
    }

    // ── Different input → different HMAC ──────────────

    [Fact]
    public void WrapConfigWithHmac_DifferentInput_DifferentHmac()
    {
        string w1 = CryptoHelper.WrapConfigWithHmac("{\"a\":1}");
        string w2 = CryptoHelper.WrapConfigWithHmac("{\"a\":2}");
        Assert.NotEqual(w1, w2);
    }
}
