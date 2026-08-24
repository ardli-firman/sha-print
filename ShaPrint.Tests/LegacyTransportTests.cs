using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Client;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Server;
using ShaPrint.WpfApp.Services;

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

    [Fact]
    public async Task PrintReceiver_EnvelopeSuccess_ReturnsAcceptedAcknowledgementWithCorrelation()
    {
        PrintReceiver receiver = CreateReceiver(printerExists: true, spoolerAccepts: true);
        LegacyAcknowledgement acknowledgement = await receiver.ProcessLegacyEnvelopePrintAsync(CreateEnvelope(123), "loopback", CancellationToken.None);

        Assert.Equal(123, acknowledgement.CorrelationId);
        Assert.Equal(LegacyAcknowledgementStatus.Accepted, acknowledgement.Status);

        using var stream = new MemoryStream();
        await LegacyAcknowledgementCodec.WriteFramedAsync(stream, acknowledgement, CancellationToken.None);
        stream.Position = 0;
        Assert.Equal(acknowledgement, await LegacyAcknowledgementCodec.ReadFramedAsync(stream, CancellationToken.None));
    }

    [Fact]
    public async Task PrintReceiver_InvalidPayload_ReturnsInvalidPayload()
    {
        PrintReceiver receiver = CreateReceiver(printerExists: true, spoolerAccepts: true);
        var envelope = new LegacyEnvelope(LegacyProtocolVersion.Current, LegacyMessageType.PrintJob, 124, new byte[] { 1 });

        LegacyAcknowledgement acknowledgement = await receiver.ProcessLegacyEnvelopePrintAsync(envelope, "loopback", CancellationToken.None);

        Assert.Equal(LegacyAcknowledgementStatus.InvalidPayload, acknowledgement.Status);
        Assert.Equal(124, acknowledgement.CorrelationId);
    }

    [Fact]
    public async Task PrintReceiver_MissingPrinterAndSpoolerFailure_MapToActionableStatuses()
    {
        LegacyAcknowledgement unavailable = await CreateReceiver(printerExists: false, spoolerAccepts: true)
            .ProcessLegacyEnvelopePrintAsync(CreateEnvelope(125), "loopback", CancellationToken.None);
        LegacyAcknowledgement rejected = await CreateReceiver(printerExists: true, spoolerAccepts: false)
            .ProcessLegacyEnvelopePrintAsync(CreateEnvelope(126), "loopback", CancellationToken.None);

        Assert.Equal(LegacyAcknowledgementStatus.TargetUnavailable, unavailable.Status);
        Assert.Equal(LegacyAcknowledgementStatus.SpoolerRejected, rejected.Status);
    }

    [Fact]
    public async Task PrintReceiver_AdmissionUnavailable_ReturnsOverloadedWithoutWaiting()
    {
        var receiver = CreateReceiver(printerExists: true, spoolerAccepts: true, capacity: 0);

        LegacyAcknowledgement acknowledgement = await receiver.ProcessLegacyEnvelopePrintAsync(CreateEnvelope(127), "loopback", CancellationToken.None);

        Assert.Equal(LegacyAcknowledgementStatus.Overloaded, acknowledgement.Status);
    }

    [Fact]
    public async Task EnvelopeHeader_PreservesCorrelationBeforeMalformedPayloadIsRejected()
    {
        byte[] header = new byte[LegacyEnvelopeCodec.HeaderLength];
        BitConverter.GetBytes(LegacyEnvelopeCodec.Magic).CopyTo(header, 0);
        header[sizeof(uint)] = LegacyProtocolVersion.Current;
        header[sizeof(uint) + sizeof(byte)] = (byte)LegacyMessageType.PrintJob;
        BitConverter.GetBytes(128L).CopyTo(header, sizeof(uint) + sizeof(byte) + sizeof(byte));
        BitConverter.GetBytes(LegacyEnvelopeCodec.MaxPayloadBytes + 1).CopyTo(header, LegacyEnvelopeCodec.HeaderLength - sizeof(int));
        using var stream = new MemoryStream(header, sizeof(uint), header.Length - sizeof(uint), writable: false);

        LegacyEnvelopeHeader parsed = await LegacyEnvelopeCodec.ReadHeaderAfterMagicAsync(stream, header.AsMemory(0, sizeof(uint)), CancellationToken.None);

        Assert.Equal(128, parsed.CorrelationId);
        await Assert.ThrowsAsync<InvalidDataException>(() => LegacyEnvelopeCodec.ReadPayloadAsync(Stream.Null, parsed, CancellationToken.None));
    }

    [Fact]
    public void EnvelopePayloadLimit_AccommodatesTheMaximumSerializedPrintJob()
        => Assert.Equal(PrintJobPayload.MaxWireBytes, LegacyEnvelopeCodec.MaxPayloadBytes);

    [Fact]
    public async Task PrintReceiver_PrinterLookupDeadline_ReturnsTimeoutWithoutBlockingRequest()
    {
        using var gate = new ManualResetEventSlim(false);
        var receiver = CreateReceiver(printerExists: true, spoolerAccepts: true);
        receiver.PrinterExists = _ =>
        {
            gate.Wait();
            return true;
        };
        using var cancellation = new CancellationTokenSource(TimeSpan.FromMilliseconds(25));

        LegacyAcknowledgement acknowledgement = await receiver.ProcessLegacyEnvelopePrintAsync(CreateEnvelope(129), "loopback", cancellation.Token);
        gate.Set();

        Assert.Equal(LegacyAcknowledgementStatus.Timeout, acknowledgement.Status);
    }

    [Fact]
    public async Task PrintReceiver_Timeout_DefersSpoolBufferCleanupUntilUnderlyingTaskCompletes()
    {
        var spoolerCompletion = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        byte[]? capturedBuffer = null;
        var receiver = CreateReceiver(printerExists: true, spoolerAccepts: true);
        receiver.SubmitPrintAsync = (_, data, _) =>
        {
            capturedBuffer = data;
            return spoolerCompletion.Task;
        };
        using var cancellation = new CancellationTokenSource(TimeSpan.FromMilliseconds(25));

        LegacyAcknowledgement acknowledgement = await receiver.ProcessLegacyEnvelopePrintAsync(CreateEnvelope(130), "loopback", cancellation.Token);

        Assert.Equal(LegacyAcknowledgementStatus.Timeout, acknowledgement.Status);
        Assert.NotNull(capturedBuffer);
        Assert.Contains(capturedBuffer!, value => value != 0);

        spoolerCompletion.SetResult(true);
        for (int attempt = 0; attempt < 20 && capturedBuffer!.Any(value => value != 0); attempt++)
            await Task.Delay(10);

        Assert.All(capturedBuffer!, value => Assert.Equal(0, value));
    }

    private static PrintReceiver CreateReceiver(bool printerExists, bool spoolerAccepts, int capacity = 1)
    {
        var receiver = new PrintReceiver(new NullNotificationService())
        {
            PrinterExists = _ => printerExists,
            SubmitPrintAsync = (_, _, _) => Task.FromResult(spoolerAccepts)
        };
        receiver.InitializeLegacyTransportForTests(capacity);
        return receiver;
    }

    private static LegacyEnvelope CreateEnvelope(long correlationId)
    {
        byte[] wire = PrintJobPayload.Serialize(new PrintJobPayload
        {
            TargetPrinterName = "Test Printer",
            DocumentName = "test.pdf",
            SpoolData = new byte[] { 1, 2, 3 }
        });
        return new LegacyEnvelope(LegacyProtocolVersion.Current, LegacyMessageType.PrintJob, correlationId, wire);
    }

    private sealed class NullNotificationService : INotificationService
    {
        public void ShowPrintJobCompleted(string documentName, string printerName) { }
        public void ShowPrintJobFailed(string documentName, string printerName, string reason) { }
        public void ShowClientConnected(string clientAddress) { }
        public void ShowClientDisconnected(string clientAddress) { }
        public void ShowScanCompleted(string fileName) { }
        public void ShowScanFailed(string errorMessage) { }
        public void ShowPrinterError(string printerName, string errorDescription) { }
        public void ShowSecurityAlert(string message, string detail) { }
        public void ShowToast(string title, string body, ToastAction? action = null) { }
    }
}
