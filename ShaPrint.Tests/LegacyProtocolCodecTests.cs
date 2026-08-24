using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Tests;

public class LegacyProtocolCodecTests
{
    [Fact]
    public void Envelope_RoundTrip_PreservesVersionTypeCorrelationAndPayload()
    {
        var original = new LegacyEnvelope(
            LegacyProtocolVersion.Current,
            LegacyMessageType.PrintJob,
            CorrelationId: 42,
            Payload: new byte[] { 0x10, 0x20, 0x30 });

        byte[] wire = LegacyEnvelopeCodec.Write(original);
        LegacyEnvelope recovered = LegacyEnvelopeCodec.Read(wire);

        Assert.Equal(original.Version, recovered.Version);
        Assert.Equal(original.MessageType, recovered.MessageType);
        Assert.Equal(original.CorrelationId, recovered.CorrelationId);
        Assert.Equal(original.Payload, recovered.Payload);
    }

    [Fact]
    public void Envelope_UnknownVersion_FailsClosed()
    {
        byte[] wire = LegacyEnvelopeCodec.Write(new LegacyEnvelope(
            LegacyProtocolVersion.Current,
            LegacyMessageType.PrintJob,
            CorrelationId: 1,
            Payload: Array.Empty<byte>()));
        wire[sizeof(uint)] = 99;

        Assert.Throws<InvalidDataException>(() => LegacyEnvelopeCodec.Read(wire));
    }

    [Fact]
    public void Envelope_UnknownMessageType_FailsClosed()
    {
        byte[] wire = LegacyEnvelopeCodec.Write(new LegacyEnvelope(
            LegacyProtocolVersion.Current,
            LegacyMessageType.PrintJob,
            CorrelationId: 1,
            Payload: Array.Empty<byte>()));
        wire[sizeof(uint) + sizeof(byte)] = 99;

        Assert.Throws<InvalidDataException>(() => LegacyEnvelopeCodec.Read(wire));
    }

    [Fact]
    public void Envelope_TruncatedFrame_FailsClosed()
    {
        byte[] wire = LegacyEnvelopeCodec.Write(new LegacyEnvelope(
            LegacyProtocolVersion.Current,
            LegacyMessageType.PrintJob,
            CorrelationId: 1,
            Payload: new byte[] { 0x01 }));

        Assert.Throws<InvalidDataException>(() => LegacyEnvelopeCodec.Read(wire[..^1]));
    }

    [Fact]
    public void Envelope_OversizedPayload_FailsBeforeAllocation()
    {
        byte[] wire = LegacyEnvelopeCodec.Write(new LegacyEnvelope(
            LegacyProtocolVersion.Current,
            LegacyMessageType.PrintJob,
            CorrelationId: 1,
            Payload: Array.Empty<byte>()));
        BitConverter.GetBytes(Constants.MaxPrintJobBytes + 1).CopyTo(
            wire, sizeof(uint) + sizeof(byte) + sizeof(byte) + sizeof(long));

        Assert.Throws<InvalidDataException>(() => LegacyEnvelopeCodec.Read(wire));
    }

    [Fact]
    public void Acknowledgement_RoundTrip_PreservesCorrelationStatusAndSanitizedMessage()
    {
        var original = new LegacyAcknowledgement(
            CorrelationId: 987654321,
            Status: LegacyAcknowledgementStatus.TargetUnavailable,
            Message: "Printer\r\nnot available\0");

        byte[] wire = LegacyAcknowledgementCodec.Write(original);
        LegacyAcknowledgement recovered = LegacyAcknowledgementCodec.Read(wire);

        Assert.Equal(original.CorrelationId, recovered.CorrelationId);
        Assert.Equal(original.Status, recovered.Status);
        Assert.Equal("Printer not available", recovered.Message);
    }

    [Fact]
    public void Acknowledgement_TamperedCiphertext_ThrowsCryptographicException()
    {
        byte[] wire = LegacyAcknowledgementCodec.Write(new LegacyAcknowledgement(
            CorrelationId: 7,
            Status: LegacyAcknowledgementStatus.Accepted,
            Message: "Accepted"));
        wire[^1] ^= 0xff;

        Assert.ThrowsAny<CryptographicException>(() => LegacyAcknowledgementCodec.Read(wire));
    }

    [Fact]
    public void Acknowledgement_ShortCiphertext_ThrowsInvalidDataException()
    {
        Assert.Throws<InvalidDataException>(() =>
            LegacyAcknowledgementCodec.Read(new byte[27]));
    }

    [Fact]
    public void Acknowledgement_TruncatedMessage_ThrowsInvalidDataException()
    {
        byte[] wire = EncryptAcknowledgementPlaintext(writer =>
        {
            writer.Write(LegacyProtocolVersion.Current);
            writer.Write(1L);
            writer.Write((byte)LegacyAcknowledgementStatus.Accepted);
            writer.Write((byte)5);
            writer.Write((byte)'x');
        });

        Assert.Throws<InvalidDataException>(() => LegacyAcknowledgementCodec.Read(wire));
    }

    [Fact]
    public void Acknowledgement_OversizedMessageLength_ThrowsBeforeAllocation()
    {
        byte[] wire = EncryptAcknowledgementPlaintext(writer =>
        {
            writer.Write(LegacyProtocolVersion.Current);
            writer.Write(1L);
            writer.Write((byte)LegacyAcknowledgementStatus.Accepted);
            writer.Write((byte)0x81);
            writer.Write((byte)0x04); // 513 in BinaryWriter's 7-bit integer encoding
            writer.Write(new byte[513]);
        });

        Assert.Throws<InvalidDataException>(() => LegacyAcknowledgementCodec.Read(wire));
    }

    [Fact]
    public void Acknowledgement_MalformedMessageLength_ThrowsInvalidDataException()
    {
        byte[] wire = EncryptAcknowledgementPlaintext(writer =>
        {
            writer.Write(LegacyProtocolVersion.Current);
            writer.Write(1L);
            writer.Write((byte)LegacyAcknowledgementStatus.Accepted);
            writer.Write(new byte[] { 0xff, 0xff, 0xff, 0xff, 0xff });
        });

        Assert.Throws<InvalidDataException>(() => LegacyAcknowledgementCodec.Read(wire));
    }

    [Fact]
    public void Acknowledgement_UnknownVersion_ThrowsInvalidDataException()
    {
        byte[] wire = EncryptAcknowledgementPlaintext(writer =>
        {
            writer.Write((byte)2);
            writer.Write(1L);
            writer.Write((byte)LegacyAcknowledgementStatus.Accepted);
            writer.Write(string.Empty);
        });

        Assert.Throws<InvalidDataException>(() => LegacyAcknowledgementCodec.Read(wire));
    }

    [Fact]
    public void Acknowledgement_UnknownStatus_ThrowsInvalidDataException()
    {
        byte[] wire = EncryptAcknowledgementPlaintext(writer =>
        {
            writer.Write(LegacyProtocolVersion.Current);
            writer.Write(1L);
            writer.Write((byte)99);
            writer.Write(string.Empty);
        });

        Assert.Throws<InvalidDataException>(() => LegacyAcknowledgementCodec.Read(wire));
    }

    [Fact]
    public void Acknowledgement_TrailingPlaintextBytes_ThrowsInvalidDataException()
    {
        byte[] wire = EncryptAcknowledgementPlaintext(writer =>
        {
            writer.Write(LegacyProtocolVersion.Current);
            writer.Write(1L);
            writer.Write((byte)LegacyAcknowledgementStatus.Accepted);
            writer.Write(string.Empty);
            writer.Write((byte)0x01);
        });

        Assert.Throws<InvalidDataException>(() => LegacyAcknowledgementCodec.Read(wire));
    }

    [Fact]
    public void PrintJobPayload_LegacyV2Payload_RemainsReadable()
    {
        byte[] spoolData = Encoding.UTF8.GetBytes("legacy data");
        byte[] innerPayload;
        using (var inner = new MemoryStream())
        using (var writer = new BinaryWriter(inner, Encoding.UTF8, leaveOpen: true))
        {
            writer.Write("LegacyPrinter");
            writer.Write(spoolData.Length);
            writer.Write(spoolData);
            writer.Flush();
            innerPayload = inner.ToArray();
        }

        byte[] encrypted = CryptoHelper.EncryptAesGcm(innerPayload);
        using var stream = new MemoryStream(encrypted);
        using var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);

        PrintJobPayload payload = PrintJobPayload.ReadInternal(reader, encrypted.Length);

        Assert.Equal("LegacyPrinter", payload.TargetPrinterName);
        Assert.Empty(payload.DocumentName);
        Assert.Equal(spoolData, payload.SpoolData);
    }

    private static byte[] EncryptAcknowledgementPlaintext(Action<BinaryWriter> writePlaintext)
    {
        using var stream = new MemoryStream();
        using (var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true))
        {
            writePlaintext(writer);
            writer.Flush();
        }

        return CryptoHelper.EncryptAesGcm(stream.ToArray());
    }
}
