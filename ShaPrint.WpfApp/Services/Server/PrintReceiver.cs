using ShaPrint.WpfApp.Services;

using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
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
        private readonly ScannerService _scannerService = new ScannerService();
        private readonly INotificationService _notificationService;
        private readonly Action<JobHistoryEntry>? _onJobLog;
        private readonly Action<ServerErrorEntry>? _onErrorLog;

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
            Task.Run(() => AcceptLoopAsync(_cts.Token));
        }

        public void Stop()
        {
            _cts?.Cancel();
            _listener?.Stop();
            try { _concurrencyLimit?.Dispose(); } catch { }
        }

        private async Task AcceptLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    var client = await _listener!.AcceptTcpClientAsync(token);
                    _ = HandleClientThrottledAsync(client, token);
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
            // Enforce concurrent connection limit
            if (!await _concurrencyLimit!.WaitAsync(TimeSpan.FromSeconds(5), token))
            {
                AppLogger.Log($"[SERVER] Rejecting connection — server at max concurrency ({Constants.MaxConcurrentPrintJobs}).");
                try { client.Close(); } catch { }
                return;
            }

            try
            {
                await HandleClientAsync(client, token);
            }
            finally
            {
                _concurrencyLimit.Release();
            }
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
                        var reader = new BinaryReader(stream, System.Text.Encoding.UTF8, leaveOpen: true);
                        int firstInt = reader.ReadInt32();

                        if (firstInt == Constants.PacketTypeScan) // 0x00000002
                        {
                            await HandleScanRequestAsync(stream, remoteIp, token);
                        }
                        else
                        {
                            int encryptedLength;
                            if (firstInt == Constants.PacketTypePrint) // 0x00000001
                            {
                                encryptedLength = reader.ReadInt32();
                            }
                            else if (firstInt >= 28) // Legacy client sending print job directly
                            {
                                encryptedLength = firstInt;
                            }
                            else
                            {
                                throw new InvalidDataException($"Invalid packet type header received: {firstInt}");
                            }

                            var payload = PrintJobPayload.ReadInternal(reader, encryptedLength);
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
                                bool printed = SpoolerApi.PrintRawData(payload.TargetPrinterName, payload.SpoolData, docName);
                                
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
    }
}
