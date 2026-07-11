using ShaPrint.Core;
using ShaPrint.Core.Network;
using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using Xunit;

namespace ShaPrint.Tests;

public class ScanPayloadTests
{
    // ── ScanRequestPayload Round-trip ────────────────

    [Fact]
    public async Task RoundTrip_ScanRequest_ReturnsOriginalData()
    {
        var original = new ScanRequestPayload
        {
            TargetScannerName = "Canon CanoScan Lide 300",
            Dpi = 300,
            ColorMode = 1,
            Format = "PNG"
        };

        using var ms = new MemoryStream();
        await ScanRequestPayload.WriteAsync(ms, original);
        ms.Position = 0;

        var recovered = await ScanRequestPayload.ReadAsync(ms);

        Assert.Equal(original.TargetScannerName, recovered.TargetScannerName);
        Assert.Equal(original.Dpi, recovered.Dpi);
        Assert.Equal(original.ColorMode, recovered.ColorMode);
        Assert.Equal(original.Format, recovered.Format);
    }

    [Fact]
    public async Task WriteAsync_EmptyScannerName_Throws()
    {
        var payload = new ScanRequestPayload
        {
            TargetScannerName = "",
            Dpi = 150
        };

        using var ms = new MemoryStream();
        await Assert.ThrowsAsync<ArgumentException>(
            () => ScanRequestPayload.WriteAsync(ms, payload));
    }

    // ── ScanResponsePayload Round-trip ───────────────

    [Fact]
    public async Task RoundTrip_ScanResponse_ReturnsOriginalData()
    {
        byte[] mockFile = Encoding.UTF8.GetBytes("Fake JPEG image binary stream");
        var original = new ScanResponsePayload
        {
            Success = true,
            ErrorMessage = string.Empty,
            FileBytes = mockFile
        };

        using var ms = new MemoryStream();
        await ScanResponsePayload.WriteAsync(ms, original);
        ms.Position = 0;

        var recovered = await ScanResponsePayload.ReadAsync(ms);

        Assert.True(recovered.Success);
        Assert.Equal(original.ErrorMessage, recovered.ErrorMessage);
        Assert.Equal(original.FileBytes, recovered.FileBytes);
    }

    [Fact]
    public async Task RoundTrip_ScanResponse_FailedScan_Works()
    {
        var original = new ScanResponsePayload
        {
            Success = false,
            ErrorMessage = "Scanner device is offline.",
            FileBytes = Array.Empty<byte>()
        };

        using var ms = new MemoryStream();
        await ScanResponsePayload.WriteAsync(ms, original);
        ms.Position = 0;

        var recovered = await ScanResponsePayload.ReadAsync(ms);

        Assert.False(recovered.Success);
        Assert.Equal(original.ErrorMessage, recovered.ErrorMessage);
        Assert.Empty(recovered.FileBytes);
    }

    // ── Tampering protection ─────────────────────────

    [Fact]
    public async Task ScanResponse_TamperedPayload_Throws()
    {
        var original = new ScanResponsePayload
        {
            Success = true,
            ErrorMessage = "",
            FileBytes = new byte[] { 1, 2, 3, 4, 5 }
        };

        using var ms = new MemoryStream();
        await ScanResponsePayload.WriteAsync(ms, original);

        byte[] wireData = ms.ToArray();
        // Tamper with the encrypted bytes (skip the 4-byte length prefix)
        int tamperOffset = 4 + 2; 
        if (tamperOffset < wireData.Length)
            wireData[tamperOffset] ^= 0x5A;

        using var tamperedStream = new MemoryStream(wireData);
        await Assert.ThrowsAsync<InvalidDataException>(
            () => ScanResponsePayload.ReadAsync(tamperedStream));
    }
}
