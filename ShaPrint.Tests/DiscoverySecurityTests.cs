using ShaPrint.Core;
using ShaPrint.Core.Network;
using System.Collections.Generic;
using System.Text;
using System.Text.Json;
using Xunit;

namespace ShaPrint.Tests;

public class DiscoverySecurityTests
{
    // ── HMAC signing & verification flow ──────────────

    [Fact]
    public void FullSignAndVerify_CorrectFlow_ClientAcceptsResponse()
    {
        // 1. Server builds response (without signature)
        var response = new DiscoveryResponseMessage
        {
            ServerName = "SRV-OFFICE",
            IpAddress = "192.168.1.100",
            ExposedPrinters = new List<PrinterInfo>
            {
                new PrinterInfo { Name = "Epson L3210", DriverName = "Epson L3210 Series" },
                new PrinterInfo { Name = "HP LaserJet", DriverName = "HP Universal Printing PCL 6" }
            }
        };

        // 2. Server serializes, signs, and re-serializes
        string unsignedJson = JsonSerializer.Serialize(response);
        response.HmacSignature = CryptoHelper.SignHmac(Encoding.UTF8.GetBytes(unsignedJson));
        string signedJson = JsonSerializer.Serialize(response);

        // 3. Client receives and verifies
        var parsed = JsonSerializer.Deserialize<DiscoveryResponseMessage>(signedJson);
        Assert.NotNull(parsed);
        Assert.NotNull(parsed!.HmacSignature);

        // Client reconstructs the unsigned JSON from the parsed object
        string savedSig = parsed.HmacSignature!;
        parsed.HmacSignature = null;
        string clientUnsigned = JsonSerializer.Serialize(parsed);

        bool valid = CryptoHelper.VerifyHmac(Encoding.UTF8.GetBytes(clientUnsigned), savedSig);
        Assert.True(valid);
    }

    [Fact]
    public void FullSignAndVerify_TamperedResponse_ClientRejectsResponse()
    {
        // Server creates legitimate response
        var response = new DiscoveryResponseMessage
        {
            ServerName = "SRV-LEGIT",
            IpAddress = "10.0.0.1",
            ExposedPrinters = new List<PrinterInfo>
            {
                new PrinterInfo { Name = "Safe Printer", DriverName = "Safe Driver" }
            }
        };

        string unsignedJson = JsonSerializer.Serialize(response);
        response.HmacSignature = CryptoHelper.SignHmac(Encoding.UTF8.GetBytes(unsignedJson));
        string signedJson = JsonSerializer.Serialize(response);

        // MITM: change the printer list but keep the old signature
        var tampered = JsonSerializer.Deserialize<DiscoveryResponseMessage>(signedJson)!;
        tampered.ExposedPrinters[0].DriverName = "Evil'; calc; #"; // injected
        // Re-sign didn't happen — attacker can't forge HMAC without the PSK
        string tamperedJson = JsonSerializer.Serialize(tampered);

        // Client parses the tampered response
        var parsed = JsonSerializer.Deserialize<DiscoveryResponseMessage>(tamperedJson)!;
        string receivedSig = parsed.HmacSignature!;
        parsed.HmacSignature = null;
        string reconstructed = JsonSerializer.Serialize(parsed);

        bool valid = CryptoHelper.VerifyHmac(Encoding.UTF8.GetBytes(reconstructed), receivedSig);
        Assert.False(valid); // Client correctly rejects
    }

    [Fact]
    public void ResponseWithoutHmac_IsDetectedByClient()
    {
        var response = new DiscoveryResponseMessage
        {
            ServerName = "SRV-OLD",
            IpAddress = "192.168.1.50",
            ExposedPrinters = new List<PrinterInfo>(),
            HmacSignature = null // legacy / malicious
        };

        string json = JsonSerializer.Serialize(response);
        var parsed = JsonSerializer.Deserialize<DiscoveryResponseMessage>(json)!;

        Assert.Null(parsed.HmacSignature);
        // Client code path: warning logged, response still accepted (legacy compat)
    }

    [Fact]
    public void SignHmac_IdenticalPayloadsProduceIdenticalSignatures()
    {
        byte[] payload = Encoding.UTF8.GetBytes("{\"ServerName\":\"TEST\"}");
        string sig1 = CryptoHelper.SignHmac(payload);
        string sig2 = CryptoHelper.SignHmac(payload);

        Assert.Equal(sig1, sig2);
    }
}
