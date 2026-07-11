using ShaPrint.Core;
using ShaPrint.Core.Network;
using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using Xunit;

namespace ShaPrint.Tests;

public class PrintJobPayloadTests
{
    // ── Basic serialization round-trip ───────────────

    [Fact]
    public async Task RoundTrip_NormalPayload_ReturnsOriginalData()
    {
        var original = new PrintJobPayload
        {
            TargetPrinterName = "Epson L3210 Series",
            DocumentName = "Invoice.pdf",
            SpoolData = Encoding.UTF8.GetBytes("RAW spool data for the printer")
        };

        using var ms = new MemoryStream();

        await PrintJobPayload.WriteAsync(ms, original);
        ms.Position = 0;

        var recovered = await PrintJobPayload.ReadAsync(ms);

        Assert.Equal(original.TargetPrinterName, recovered.TargetPrinterName);
        Assert.Equal(original.DocumentName, recovered.DocumentName);
        Assert.Equal(original.SpoolData, recovered.SpoolData);
    }

    // ── Encryption is actually applied ───────────────

    [Fact]
    public async Task WireFormat_DoesNotContainPlaintext()
    {
        var original = new PrintJobPayload
        {
            TargetPrinterName = "SecretPrinter",
            DocumentName = "Classified.docx",
            SpoolData = Encoding.UTF8.GetBytes("CLASSIFIED DOCUMENT")
        };

        using var ms = new MemoryStream();
        await PrintJobPayload.WriteAsync(ms, original);
        byte[] wireData = ms.ToArray();

        string wireText = Encoding.UTF8.GetString(wireData);

        // The plaintext printer name and spool data should NOT appear on the wire
        Assert.DoesNotContain("SecretPrinter", wireText);
        Assert.DoesNotContain("CLASSIFIED", wireText);
    }

    // ── Validation: empty printer name ───────────────

    [Fact]
    public async Task WriteAsync_EmptyPrinterName_Throws()
    {
        var payload = new PrintJobPayload
        {
            TargetPrinterName = "",
            SpoolData = new byte[] { 1, 2, 3 }
        };

        using var ms = new MemoryStream();
        await Assert.ThrowsAsync<ArgumentException>(
            () => PrintJobPayload.WriteAsync(ms, payload));
    }

    // ── Validation: excessive printer name ────────────

    [Fact]
    public async Task WriteAsync_PrinterNameTooLong_Throws()
    {
        var payload = new PrintJobPayload
        {
            TargetPrinterName = new string('X', Constants.MaxTargetPrinterNameBytes + 1),
            SpoolData = new byte[] { 1 }
        };

        using var ms = new MemoryStream();
        await Assert.ThrowsAsync<ArgumentException>(
            () => PrintJobPayload.WriteAsync(ms, payload));
    }

    // ── Validation: excessive spool data ──────────────

    [Fact]
    public async Task WriteAsync_SpoolDataTooLarge_Throws()
    {
        var payload = new PrintJobPayload
        {
            TargetPrinterName = "OK",
            SpoolData = new byte[Constants.MaxPrintJobBytes + 1]
        };

        using var ms = new MemoryStream();
        await Assert.ThrowsAsync<ArgumentException>(
            () => PrintJobPayload.WriteAsync(ms, payload));
    }

    // ── Validation: negative data length on wire ──────

    [Fact]
    public async Task ReadAsync_NegativeLengthOnWire_Throws()
    {
        // Craft a malicious wire format: write a negative encryptedBlob length
        using var ms = new MemoryStream();
        using (var bw = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
        {
            bw.Write(-1); // negative length
            // No payload bytes follow
        }
        ms.Position = 0;

        await Assert.ThrowsAsync<InvalidDataException>(
            () => PrintJobPayload.ReadAsync(ms));
    }

    // ── Validation: excessive encrypted blob ──────────

    [Fact]
    public async Task ReadAsync_ExcessiveEncryptedLength_Throws()
    {
        using var ms = new MemoryStream();
        using (var bw = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
        {
            bw.Write(Constants.MaxPrintJobBytes + 2048); // too large
            // No actual payload — reading will fail on short stream
        }
        ms.Position = 0;

        await Assert.ThrowsAsync<InvalidDataException>(
            () => PrintJobPayload.ReadAsync(ms));
    }

    // ── Validation: truncated encrypted blob ──────────

    [Fact]
    public async Task ReadAsync_TruncatedPayload_Throws()
    {
        using var ms = new MemoryStream();
        using (var bw = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
        {
            bw.Write(100); // claim 100 bytes follow
            bw.Write(new byte[10]); // but only 10 provided
        }
        ms.Position = 0;

        await Assert.ThrowsAsync<InvalidDataException>(
            () => PrintJobPayload.ReadAsync(ms));
    }

    // ── Large but valid payload ──────────────────────

    [Fact]
    public async Task RoundTrip_LargePayload_Works()
    {
        byte[] largeData = new byte[1_000_000];
        RandomNumberGenerator.Fill(largeData);

        var original = new PrintJobPayload
        {
            TargetPrinterName = "Large Job Printer",
            DocumentName = "HugeReport.xlsx",
            SpoolData = largeData
        };

        using var ms = new MemoryStream();
        await PrintJobPayload.WriteAsync(ms, original);
        ms.Position = 0;

        var recovered = await PrintJobPayload.ReadAsync(ms);

        Assert.Equal(original.TargetPrinterName, recovered.TargetPrinterName);
        Assert.Equal(original.DocumentName, recovered.DocumentName);
        Assert.Equal(original.SpoolData, recovered.SpoolData);
    }

    // ── Tampered wire fails authentication ────────────

    [Fact]
    public async Task ReadAsync_TamperedEncryptedBlob_Throws()
    {
        var original = new PrintJobPayload
        {
            TargetPrinterName = "Target",
            DocumentName = "secret.txt",
            SpoolData = Encoding.UTF8.GetBytes("secret print data")
        };

        using var ms = new MemoryStream();
        await PrintJobPayload.WriteAsync(ms, original);

        // Tamper with the encrypted bytes (skip the 4-byte length prefix)
        byte[] buffer = ms.ToArray();
        // The first 4 bytes are the length; tamper somewhere after that
        int tamperOffset = 4 + 2; // past length + into encrypted blob
        if (tamperOffset < buffer.Length)
            buffer[tamperOffset] ^= 0xFF;

        using var tamperedStream = new MemoryStream(buffer);
        await Assert.ThrowsAsync<InvalidDataException>(
            () => PrintJobPayload.ReadAsync(tamperedStream));
    }

    [Fact]
    public async Task ReadAsync_DocumentNameTooLong_TruncatesTo1024Characters()
    {
        var original = new PrintJobPayload
        {
            TargetPrinterName = "Printer",
            DocumentName = new string('A', 2000), // 2000 characters
            SpoolData = Encoding.UTF8.GetBytes("Data")
        };

        using var ms = new MemoryStream();
        await PrintJobPayload.WriteAsync(ms, original);
        ms.Position = 0;

        var recovered = await PrintJobPayload.ReadAsync(ms);

        Assert.Equal(1024, recovered.DocumentName.Length);
        Assert.Equal(new string('A', 1024), recovered.DocumentName);
    }

    [Fact]
    public async Task ReadAsync_LegacyPayloadWithoutDocumentName_ParsesSuccessfullyWithEmptyDocumentName()
    {
        var printerName = "LegacyPrinter";
        var spoolData = Encoding.UTF8.GetBytes("Legacy Spool Data");

        // Manually build legacy inner payload
        byte[] innerPayload;
        using (var ms = new MemoryStream())
        using (var bw = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
        {
            bw.Write(printerName);
            bw.Write(spoolData.Length);
            bw.Write(spoolData);
            bw.Flush();
            innerPayload = ms.ToArray();
        }

        // Encrypt with AES-GCM
        byte[] encryptedBlob = CryptoHelper.EncryptAesGcm(innerPayload);

        // Build the wire payload
        using var wireMs = new MemoryStream();
        using (var bw = new BinaryWriter(wireMs, Encoding.UTF8, leaveOpen: true))
        {
            bw.Write(encryptedBlob.Length);
            bw.Write(encryptedBlob);
            bw.Flush();
        }

        wireMs.Position = 0;

        // Act
        var payload = await PrintJobPayload.ReadAsync(wireMs);

        // Assert
        Assert.Equal(printerName, payload.TargetPrinterName);
        Assert.Equal(string.Empty, payload.DocumentName);
        Assert.Equal(spoolData, payload.SpoolData);
    }
}
