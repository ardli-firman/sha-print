using ShaPrint.WpfApp.Services;

using System;
using System.Buffers.Binary;
using System.Collections.Concurrent;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Server
{
    public class PrintReceiver
    {
        private TcpListener? _listener;
        private CancellationTokenSource? _cts;
        private SemaphoreSlim? _concurrencyLimit;
        private readonly ConcurrentDictionary<int, Task> _activeHandlers = new();
        private int _nextHandlerId;
        private readonly ScannerService _scannerService = new ScannerService();
        private readonly INotificationService _notificationService;
        private readonly Action<JobHistoryEntry>? _onJobLog;
        private readonly Action<ServerErrorEntry>? _onErrorLog;

        // Driver provisioning (injected from server startup)
        private DriverPackageService? _driverPackageService;
        private volatile bool _driverSharingEnabled = true;

        // Test seams also keep transport/status mapping independent of physical
        // Windows devices. Production defaults continue to use the native API.
        internal Func<string, bool> PrinterExists { get; set; } = name => SpoolerApi.GetLocalPrinters().Contains(name, StringComparer.OrdinalIgnoreCase);
        internal Func<string, byte[], string, Task<bool>> SubmitPrintAsync { get; set; } =
            (printer, data, document) => SpoolerApi.PrintRawDataAsync(printer, data, document);

        public void SetDriverPackageService(DriverPackageService service)
            => _driverPackageService = service;

        public void SetDriverSharingEnabled(bool enabled)
            => _driverSharingEnabled = enabled;

        public PrintReceiver(INotificationService notificationService, Action<JobHistoryEntry>? onJobLog = null, Action<ServerErrorEntry>? onErrorLog = null)
        {
            _notificationService = notificationService;
            _onJobLog = onJobLog;
            _onErrorLog = onErrorLog;
        }

        public void Start()
        {
            _cts = new CancellationTokenSource();
            _concurrencyLimit = new SemaphoreSlim(Constants.MaxConcurrentPrintJobs);
            _listener = new TcpListener(IPAddress.Any, Constants.PrintTcpPort);
            _listener.Start();
            _ = AcceptLoopAsync(_cts.Token);
        }

        public void Stop()
        {
            _cts?.Cancel();
            _listener?.Stop();
            Task[] handlers = _activeHandlers.Values.ToArray();
            // Do not dispose the shared semaphore here: a handler admitted just
            // before Stop may still release it. The receiver owns one small
            // semaphore for its lifetime, while cancellation bounds its work.
            _ = Task.WhenAny(Task.WhenAll(handlers), Task.Delay(TimeSpan.FromSeconds(2)))
                .ContinueWith(_ => { }, TaskScheduler.Default);
        }

        private async Task AcceptLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    var client = await _listener!.AcceptTcpClientAsync(token);
                    int handlerId = Interlocked.Increment(ref _nextHandlerId);
                    Task handler = HandleClientThrottledAsync(client, token);
                    _activeHandlers[handlerId] = handler;
                    _ = handler.ContinueWith(completed =>
                    {
                        _activeHandlers.TryRemove(handlerId, out _);
                        if (completed.IsFaulted)
                            AppLogger.Error("[SERVER] Unhandled client handler failure", completed.Exception!);
                    }, TaskScheduler.Default);
                }
                catch (OperationCanceledException) { break; }
                catch (ObjectDisposedException) { break; }
                catch (SocketException ex)
                {
                    AppLogger.Error("[SERVER] Socket error in accept loop", ex);
                }
                catch (Exception ex)
                {
                    AppLogger.Error("[SERVER] Unexpected error in accept loop", ex);
                }
            }
        }

        private async Task HandleClientThrottledAsync(TcpClient client, CancellationToken token)
        {
            try
            {
                await HandleClientAsync(client, token);
            }
            catch (Exception ex) { AppLogger.Error("[SERVER] Client handler failed", ex); }
        }

        private async Task HandleClientAsync(TcpClient client, CancellationToken token)
        {
            using (client)
            {
                var remoteIp = "unknown";
                try
                {
                    remoteIp = ((IPEndPoint)client.Client.RemoteEndPoint!).Address.ToString();
                }
                catch { }

                AppLogger.Log($"[SERVER] Incoming TCP connection from {remoteIp}");

                using (var stream = client.GetStream())
                {
                    try
                    {
                        int firstInt;
                        byte[] firstHeader = new byte[sizeof(int)];
                        try
                        {
                            await LegacyEnvelopeCodec.ReadExactlyAsync(stream, firstHeader, token);
                            if (BinaryPrimitives.ReadUInt32LittleEndian(firstHeader) == LegacyEnvelopeCodec.Magic)
                            {
                                await HandleEnvelopePrintRequestAsync(stream, firstHeader, remoteIp, token);
                                return;
                            }
                            firstInt = BinaryPrimitives.ReadInt32LittleEndian(firstHeader);
                        }
                        finally
                        {
                            CryptographicOperations.ZeroMemory(firstHeader);
                        }

                        var reader = new BinaryReader(stream, System.Text.Encoding.UTF8, leaveOpen: true);

                        if (_concurrencyLimit is null || !_concurrencyLimit.Wait(0))
                        {
                            AppLogger.Log($"[SERVER] Rejecting legacy connection — server at max concurrency ({Constants.MaxConcurrentPrintJobs}).");
                            return;
                        }

                        try
                        {
                            if (firstInt == Constants.PacketTypeScan) // 0x00000002
                            {
                                await HandleScanRequestAsync(stream, remoteIp, token);
                            }
                            else if (firstInt == Constants.PacketTypeDriverPackageRequest) // 0x20
                            {
                                await HandleDriverPackageRequestAsync(stream, reader, remoteIp, token);
                            }
                            else
                            {
                            int encryptedLength;
                            if (firstInt == Constants.PacketTypePrint) // 0x00000001
                            {
                                byte[] lengthBuffer = new byte[sizeof(int)];
                                try
                                {
                                    await LegacyEnvelopeCodec.ReadExactlyAsync(stream, lengthBuffer, token);
                                    encryptedLength = BinaryPrimitives.ReadInt32LittleEndian(lengthBuffer);
                                }
                                finally { CryptographicOperations.ZeroMemory(lengthBuffer); }
                            }
                            else if (firstInt >= 28) // Legacy client sending print job directly
                            {
                                encryptedLength = firstInt;
                            }
                            else
                            {
                                throw new InvalidDataException($"Invalid packet type header received: {firstInt}");
                            }

                            if (encryptedLength < 0 || encryptedLength > Constants.MaxPrintJobBytes + 1024)
                                throw new InvalidDataException("Legacy print payload length is invalid.");
                            byte[] encrypted = new byte[encryptedLength];
                            try
                            {
                                await LegacyEnvelopeCodec.ReadExactlyAsync(stream, encrypted, token);
                                var payload = PrintJobPayload.ReadEncryptedBlob(encrypted);
                                try
                                {
                            AppLogger.Log($"[SERVER] Received payload. Printer: '{payload.TargetPrinterName}', Data size: {payload.SpoolData?.Length ?? 0} bytes.");

                            // Defense-in-depth: re-validate printer name after decryption
                            try
                            {
                                payload.TargetPrinterName = Validators.ValidatePrinterName(payload.TargetPrinterName);
                            }
                            catch (ArgumentException ex)
                            {
                                AppLogger.Error($"[SERVER] Printer name validation failed after decryption: {ex.Message}");
                                return;
                            }

                            if (!string.IsNullOrEmpty(payload.TargetPrinterName) && payload.SpoolData != null && payload.SpoolData.Length > 0)
                            {
                                string docName = !string.IsNullOrEmpty(payload.DocumentName)
                                    ? payload.DocumentName
                                    : "ShaPrint Job - " + DateTime.Now.ToString("yyyyMMdd_HHmmss");
                                AppLogger.Log($"[SERVER] Injecting {payload.SpoolData.Length} bytes into Windows Spooler for '{payload.TargetPrinterName}'");
                                bool printed = await SubmitPrintAsync(payload.TargetPrinterName, payload.SpoolData, docName);
                                
                                if (printed)
                                {
                                    AppLogger.Log($"[SERVER] SUCCESS: Print job accepted by Windows Spooler.");
                                    _notificationService.ShowPrintJobCompleted(docName, payload.TargetPrinterName);
                                    _onJobLog?.Invoke(new JobHistoryEntry
                                    {
                                        Type = "print",
                                        Document = docName,
                                        PrinterName = payload.TargetPrinterName,
                                        ClientIp = remoteIp,
                                        Status = "completed",
                                        Timestamp = DateTime.UtcNow
                                    });
                                }
                                else
                                {
                                    AppLogger.Error($"[SERVER] FAILED: Windows Spooler rejected the job. Check SpoolerApi logs.");
                                    _notificationService.ShowPrintJobFailed(docName, payload.TargetPrinterName, "Spooler rejected job");
                                    _onJobLog?.Invoke(new JobHistoryEntry
                                    {
                                        Type = "print",
                                        Document = docName,
                                        PrinterName = payload.TargetPrinterName,
                                        ClientIp = remoteIp,
                                        Status = "failed",
                                        Timestamp = DateTime.UtcNow
                                    });
                                    _onErrorLog?.Invoke(new ServerErrorEntry
                                    {
                                        Source = "PrintReceiver",
                                        Message = $"Windows Spooler rejected job '{docName}' for printer '{payload.TargetPrinterName}'",
                                        Timestamp = DateTime.UtcNow
                                    });
                                }
                            }
                            else
                            {
                                AppLogger.Error($"[SERVER] ERROR: Empty payload or missing printer name.");
                            }
                            }
                                finally
                                {
                                    CryptographicOperations.ZeroMemory(payload.SpoolData);
                                }
                            }
                            finally
                            {
                                CryptographicOperations.ZeroMemory(encrypted);
                            }
                            }
                        }
                        finally
                        {
                            _concurrencyLimit.Release();
                        }
                    }
                    catch (InvalidDataException ex)
                    {
                        AppLogger.Error($"[SERVER] Malformed payload from {remoteIp}: " + ex.Message);
                    }
                    catch (Exception ex)
                    {
                        AppLogger.Error($"[SERVER] ERROR handling print/scan job from {remoteIp}: " + ex.Message);
                    }
                }
            }
        }

        private async Task HandleEnvelopePrintRequestAsync(NetworkStream stream, byte[] magicPrefix, string remoteIp, CancellationToken token)
        {
            LegacyEnvelope? envelope = null;
            PrintJobPayload? payload = null;
            try
            {
                envelope = await LegacyEnvelopeCodec.ReadAfterMagicAsync(stream, magicPrefix, token);
                if (envelope.CorrelationId == 0 || envelope.MessageType != LegacyMessageType.PrintJob)
                    throw new InvalidDataException("Legacy envelope is not a correlated print request.");

                if (_concurrencyLimit is null || !_concurrencyLimit.Wait(0))
                {
                    await SendAcknowledgementAsync(stream, envelope.CorrelationId, LegacyAcknowledgementStatus.Overloaded,
                        "Server is busy; submit a new job after capacity is available.", token);
                    return;
                }

                try
                {
                    payload = PrintJobPayload.ReadWire(envelope.Payload);
                    payload.TargetPrinterName = Validators.ValidatePrinterName(payload.TargetPrinterName);
                    string documentName = Validators.ValidateDocumentName(payload.DocumentName);
                    if (payload.SpoolData.Length == 0)
                        throw new InvalidDataException("Print job data is empty.");

                    bool exists;
                    try { exists = PrinterExists(payload.TargetPrinterName); }
                    catch (Exception ex)
                    {
                        AppLogger.Error("[SERVER] Unable to enumerate printers for legacy request", ex);
                        exists = true;
                    }

                    if (!exists)
                    {
                        await SendAcknowledgementAsync(stream, envelope.CorrelationId, LegacyAcknowledgementStatus.TargetUnavailable,
                            "The target printer is unavailable.", token);
                        return;
                    }

                    bool printed = await SubmitPrintAsync(payload.TargetPrinterName, payload.SpoolData, documentName);
                    if (!printed)
                    {
                        await SendAcknowledgementAsync(stream, envelope.CorrelationId, LegacyAcknowledgementStatus.SpoolerRejected,
                            "Windows Spooler rejected the job.", token);
                        return;
                    }

                    _notificationService.ShowPrintJobCompleted(documentName, payload.TargetPrinterName);
                    _onJobLog?.Invoke(new JobHistoryEntry
                    {
                        Type = "print",
                        Document = documentName,
                        PrinterName = payload.TargetPrinterName,
                        ClientIp = remoteIp,
                        Status = "completed",
                        Timestamp = DateTime.UtcNow
                    });
                    await SendAcknowledgementAsync(stream, envelope.CorrelationId, LegacyAcknowledgementStatus.Accepted,
                        "Print job accepted by Windows Spooler.", token);
                }
                finally
                {
                    _concurrencyLimit.Release();
                }
            }
            catch (OperationCanceledException)
            {
                if (envelope?.CorrelationId is long correlationId && correlationId != 0)
                    await SendAcknowledgementBestEffortAsync(stream, correlationId,
                        token.IsCancellationRequested ? LegacyAcknowledgementStatus.Canceled : LegacyAcknowledgementStatus.Timeout,
                        "Print request was canceled or timed out.");
            }
            catch (InvalidDataException ex)
            {
                AppLogger.Error($"[SERVER] Malformed legacy payload from {remoteIp}: {ex.Message}");
                if (envelope?.CorrelationId is long correlationId && correlationId != 0)
                    await SendAcknowledgementBestEffortAsync(stream, correlationId, LegacyAcknowledgementStatus.InvalidPayload, "Print payload is invalid.");
            }
            catch (ArgumentException ex)
            {
                AppLogger.Error($"[SERVER] Invalid legacy print metadata from {remoteIp}: {ex.Message}");
                if (envelope?.CorrelationId is long correlationId && correlationId != 0)
                    await SendAcknowledgementBestEffortAsync(stream, correlationId, LegacyAcknowledgementStatus.InvalidPayload, "Print metadata is invalid.");
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[SERVER] Legacy print request failed for {remoteIp}", ex);
                if (envelope?.CorrelationId is long correlationId && correlationId != 0)
                    await SendAcknowledgementBestEffortAsync(stream, correlationId, LegacyAcknowledgementStatus.ServerError, "Server error while submitting print job.");
            }
            finally
            {
                if (payload?.SpoolData is { Length: > 0 } spoolData)
                    CryptographicOperations.ZeroMemory(spoolData);
                if (envelope?.Payload is { Length: > 0 } envelopePayload)
                    CryptographicOperations.ZeroMemory(envelopePayload);
            }
        }

        private static async Task SendAcknowledgementAsync(NetworkStream stream, long correlationId, LegacyAcknowledgementStatus status, string message, CancellationToken token)
            => await LegacyAcknowledgementCodec.WriteFramedAsync(stream, new LegacyAcknowledgement(correlationId, status, message), token);

        private static async Task SendAcknowledgementBestEffortAsync(NetworkStream stream, long correlationId, LegacyAcknowledgementStatus status, string message)
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2));
            try { await SendAcknowledgementAsync(stream, correlationId, status, message, timeout.Token); }
            catch { }
        }

        private async Task HandleScanRequestAsync(NetworkStream stream, string remoteIp, CancellationToken token)
        {
            try
            {
                var request = await ScanRequestPayload.ReadAsync(stream);
                AppLogger.Log($"[SERVER] Received scan request from {remoteIp} for scanner '{request.TargetScannerName}' (DPI={request.Dpi}, ColorMode={request.ColorMode}, Format={request.Format})");

                var response = new ScanResponsePayload();
                try
                {
                    string actualFormat;
                    byte[] scannedBytes = _scannerService.PerformScan(
                        request.TargetScannerName, 
                        request.Dpi, 
                        request.ColorMode, 
                        request.Format, 
                        out actualFormat);
                    response.Success = true;
                    response.FileBytes = scannedBytes;
                    response.ErrorMessage = string.Empty;

                    _onJobLog?.Invoke(new JobHistoryEntry
                    {
                        Type = "scan",
                        Document = $"Scan - {request.TargetScannerName}",
                        PrinterName = request.TargetScannerName,
                        ClientIp = remoteIp,
                        Status = "completed",
                        Timestamp = DateTime.UtcNow
                    });
                }
                catch (Exception ex)
                {
                    AppLogger.Error($"[SERVER] Scan execution failed for {remoteIp}", ex);
                    response.Success = false;
                    response.ErrorMessage = ex.Message;
                    response.FileBytes = Array.Empty<byte>();

                    _onJobLog?.Invoke(new JobHistoryEntry
                    {
                        Type = "scan",
                        Document = $"Scan - {request.TargetScannerName}",
                        PrinterName = request.TargetScannerName,
                        ClientIp = remoteIp,
                        Status = "failed",
                        Timestamp = DateTime.UtcNow
                    });
                    _onErrorLog?.Invoke(new ServerErrorEntry
                    {
                        Source = "PrintReceiver-Scan",
                        Message = $"Scan failed for scanner '{request.TargetScannerName}': {ex.Message}",
                        Timestamp = DateTime.UtcNow
                    });
                }

                AppLogger.Log($"[SERVER] Sending scan response to {remoteIp}. Success={response.Success}, Size={response.FileBytes.Length} bytes.");
                await ScanResponsePayload.WriteAsync(stream, response);
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[SERVER] Error reading/writing scan payload from {remoteIp}", ex);
            }
        }

        /// <summary>
        /// Handles a client request to download a driver package.
        /// Reads DriverPackageRequest, then streams chunks (AES-GCM encrypted, HMAC signed) back to client.
        /// </summary>
        private async Task HandleDriverPackageRequestAsync(NetworkStream stream, BinaryReader reader, string remoteIp, CancellationToken token)
        {
            try
            {
                if (!_driverSharingEnabled || _driverPackageService == null)
                {
                    await SendDriverPackageErrorAsync(stream, "Driver sharing is disabled on this server.");
                    return;
                }

                // Read request: length-prefixed JSON
                int jsonLength = reader.ReadInt32();
                if (jsonLength <= 0 || jsonLength > 10240) // max 10KB for the request
                {
                    await SendDriverPackageErrorAsync(stream, "Invalid request length.");
                    return;
                }
                byte[] jsonBytes = reader.ReadBytes(jsonLength);
                var request = System.Text.Json.JsonSerializer.Deserialize<DriverPackageRequest>(
                    System.Text.Encoding.UTF8.GetString(jsonBytes));

                if (request == null || string.IsNullOrEmpty(request.PrinterName))
                {
                    await SendDriverPackageErrorAsync(stream, "Invalid driver package request.");
                    return;
                }

                AppLogger.Log($"[SERVER] Driver package request from {remoteIp} for printer '{request.PrinterName}' (PackageId={request.DriverPackageId?[..16]}...)");

                // Resolve printer name → driver name
                string driverName;
                try
                {
                    var allPrinters = SpoolerApi.GetLocalPrintersDetailed();
                    var match = allPrinters.FirstOrDefault(x =>
                        x.Name.Equals(request.PrinterName, StringComparison.OrdinalIgnoreCase));
                    driverName = match?.DriverName ?? request.PrinterName;
                    AppLogger.Log($"[SERVER] Resolved printer '{request.PrinterName}' → driver '{driverName}' ({(match != null ? "matched" : "fallback to printer name")})");
                }
                catch (Exception ex)
                {
                    AppLogger.Error($"[SERVER] Error resolving driver name for printer '{request.PrinterName}', falling back to printer name", ex);
                    driverName = request.PrinterName;
                }

                // Get or build the package
                AppLogger.Log($"[SERVER] Fetching driver package for driver='{driverName}'...");
                var manifest = await _driverPackageService.GetDriverPackageAsync(driverName);
                if (manifest == null)
                {
                    AppLogger.Error($"[SERVER] Driver package not found for driver='{driverName}' (requested printer='{request.PrinterName}')");
                    await SendDriverPackageErrorAsync(stream, $"Driver package not found for printer '{request.PrinterName}' (driver: '{driverName}').");
                    return;
                }
                AppLogger.Log($"[SERVER] Package manifest found: SHA-256={manifest.Sha256[..16]}..., size={manifest.TotalSizeBytes:N0} bytes, inf={manifest.InfName}");

                // Verify requested package ID matches (if provided)
                if (!string.IsNullOrEmpty(request.DriverPackageId) &&
                    !request.DriverPackageId.Equals(manifest.Sha256, StringComparison.OrdinalIgnoreCase))
                {
                    AppLogger.Error($"[SERVER] PackageId mismatch: client requested '{request.DriverPackageId[..16]}...', server has '{manifest.Sha256[..16]}...'");
                    await SendDriverPackageErrorAsync(stream, "Driver package ID mismatch.");
                    return;
                }

                // Read package bytes
                AppLogger.Log($"[SERVER] Reading package bytes for SHA-256={manifest.Sha256[..16]}...");
                byte[]? packageBytes = await _driverPackageService.ReadPackageBytesAsync(manifest.Sha256);
                if (packageBytes == null || packageBytes.Length == 0)
                {
                    AppLogger.Error($"[SERVER] Failed to read package bytes for SHA-256={manifest.Sha256[..16]}... — aborting transfer.");
                    await SendDriverPackageErrorAsync(stream, "Failed to read driver package data.");
                    return;
                }
                AppLogger.Log($"[SERVER] Sending driver package to {remoteIp}: {packageBytes.Length:N0} bytes.");

                // Stream chunks
                int chunkSize = Constants.DriverPackageChunkSize;
                int totalChunks = (int)Math.Ceiling((double)packageBytes.Length / chunkSize);

                for (int i = 0; i < totalChunks; i++)
                {
                    token.ThrowIfCancellationRequested();

                    int offset = i * chunkSize;
                    int length = Math.Min(chunkSize, packageBytes.Length - offset);
                    byte[] rawChunk = new byte[length];
                    Buffer.BlockCopy(packageBytes, offset, rawChunk, 0, length);

                    // Encrypt chunk with AES-GCM
                    byte[] encryptedChunk = CryptoHelper.EncryptAesGcm(rawChunk);
                    // Sign the raw chunk with HMAC
                    string chunkHmac = CryptoHelper.SignHmac(rawChunk);

                    var chunkMsg = new DriverPackageChunk
                    {
                        ChunkIndex = i,
                        TotalChunks = totalChunks,
                        Data = encryptedChunk,
                        ChunkHmac = chunkHmac
                    };

                    // Write packet type + length-prefixed JSON
                    byte[] chunkJson = System.Text.Json.JsonSerializer.SerializeToUtf8Bytes(chunkMsg);
                    using var ms = new MemoryStream();
                    using (var writer = new BinaryWriter(ms, System.Text.Encoding.UTF8, leaveOpen: true))
                    {
                        writer.Write(Constants.PacketTypeDriverPackageChunk);
                        writer.Write(chunkJson.Length);
                        writer.Write(chunkJson);
                    }
                    byte[] packet = ms.ToArray();
                    await stream.WriteAsync(packet, token);
                    await stream.FlushAsync(token);
                }

                // Send completion message
                var completeMsg = new DriverPackageComplete
                {
                    TotalBytes = packageBytes.Length,
                    TotalChunks = totalChunks
                };

                byte[] completeJson = System.Text.Json.JsonSerializer.SerializeToUtf8Bytes(completeMsg);
                using var completeMs = new MemoryStream();
                using (var writer = new BinaryWriter(completeMs, System.Text.Encoding.UTF8, leaveOpen: true))
                {
                    writer.Write(Constants.PacketTypeDriverPackageComplete);
                    writer.Write(completeJson.Length);
                    writer.Write(completeJson);
                }
                byte[] completePacket = completeMs.ToArray();
                await stream.WriteAsync(completePacket, token);
                await stream.FlushAsync(token);

                AppLogger.Log($"[SERVER] Driver package transfer complete to {remoteIp}: {totalChunks} chunks, {packageBytes.Length} bytes.");
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[SERVER] Driver package transfer error from {remoteIp}", ex);
                try { await SendDriverPackageErrorAsync(stream, "Server error during driver package transfer."); } catch { }
            }
        }

        private async Task SendDriverPackageErrorAsync(NetworkStream stream, string message)
        {
            var errorMsg = new DriverPackageError { Message = message };
            byte[] errorJson = System.Text.Json.JsonSerializer.SerializeToUtf8Bytes(errorMsg);
            using var ms = new MemoryStream();
            using (var writer = new BinaryWriter(ms, System.Text.Encoding.UTF8, leaveOpen: true))
            {
                writer.Write(Constants.PacketTypeDriverPackageError);
                writer.Write(errorJson.Length);
                writer.Write(errorJson);
            }
            byte[] packet = ms.ToArray();
            await stream.WriteAsync(packet);
            await stream.FlushAsync();
        }
    }
}
