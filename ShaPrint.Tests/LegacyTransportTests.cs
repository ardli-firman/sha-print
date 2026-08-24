using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Client;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Tests;

public class LegacyTransportTests
{
    [Fact]
    public async Task Acknowledgement_FramedAsync_RoundTripsCorrelationAndStatus()
    {
        using var stream = new MemoryStream();
        var expected = new LegacyAcknowledgement(4815162342, LegacyAcknowledgementStatus.TargetUnavailable, "Printer is offline.");

        await LegacyAcknowledgementCodec.WriteFramedAsync(stream, expected, CancellationToken.None);
        stream.Position = 0;

        LegacyAcknowledgement actual = await LegacyAcknowledgementCodec.ReadFramedAsync(stream, CancellationToken.None);

        Assert.Equal(expected.CorrelationId, actual.CorrelationId);
        Assert.Equal(expected.Status, actual.Status);
        Assert.Equal(expected.Message, actual.Message);
    }

    [Fact]
    public async Task Acknowledgement_FramedAsync_TruncatedCiphertext_FailsClosed()
    {
        using var stream = new MemoryStream(new byte[] { 28, 0, 0, 0, 1, 2, 3 });

        await Assert.ThrowsAsync<EndOfStreamException>(() =>
            LegacyAcknowledgementCodec.ReadFramedAsync(stream, CancellationToken.None));
    }

    [Fact]
    public async Task Envelope_ReadAfterMagicAsync_UsesExactFrameBoundary()
    {
        byte[] frame = LegacyEnvelopeCodec.Write(new LegacyEnvelope(
            LegacyProtocolVersion.Current, LegacyMessageType.PrintJob, 99, new byte[] { 1, 2, 3 }));
        using var stream = new MemoryStream(frame, sizeof(uint), frame.Length - sizeof(uint), writable: false);

        LegacyEnvelope actual = await LegacyEnvelopeCodec.ReadAfterMagicAsync(
            stream, frame.AsMemory(0, sizeof(uint)), CancellationToken.None);

        Assert.Equal(99, actual.CorrelationId);
        Assert.Equal(new byte[] { 1, 2, 3 }, actual.Payload);
    }

    [Fact]
    public void PrintJobPayload_ReadWire_RejectsLengthMismatch()
    {
        Assert.Throws<InvalidDataException>(() => PrintJobPayload.ReadWire(new byte[] { 28, 0, 0, 0, 1 }));
    }

    [Fact]
    public void DocumentName_ControlCharacter_IsRejectedBeforeLogging()
    {
        Assert.Throws<ArgumentException>(() => Validators.ValidateDocumentName("invoice\r\nforged"));
    }

    [Fact]
    public async Task PipePayload_CopyHonorsCancellationWithoutReadingIndefinitely()
    {
        using var source = new MemoryStream(new byte[] { 1, 2, 3 });
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
            PipeListener.ReadBoundedPipePayloadAsync(source, cancellation.Token));
    }
}
