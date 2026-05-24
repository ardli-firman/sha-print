using System;
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
                        var payload = await PrintJobPayload.ReadAsync(stream);
                        AppLogger.Log($"[SERVER] Received payload. Printer: '{payload.TargetPrinterName}', Data size: {payload.SpoolData?.Length ?? 0} bytes.");

                        if (!string.IsNullOrEmpty(payload.TargetPrinterName) && payload.SpoolData != null && payload.SpoolData.Length > 0)
                        {
                            string docName = "ShaPrint Job - " + DateTime.Now.ToString("yyyyMMdd_HHmmss");
                            AppLogger.Log($"[SERVER] Injecting {payload.SpoolData.Length} bytes into Windows Spooler for '{payload.TargetPrinterName}'...");
                            bool printed = SpoolerApi.PrintRawData(payload.TargetPrinterName, payload.SpoolData, docName);
                            
                            if (printed)
                                AppLogger.Log($"[SERVER] SUCCESS: Print job accepted by Windows Spooler.");
                            else
                                AppLogger.Error($"[SERVER] FAILED: Windows Spooler rejected the job. Check SpoolerApi logs.");
                        }
                        else
                        {
                            AppLogger.Error($"[SERVER] ERROR: Empty payload or missing printer name.");
                        }
                    }
                    catch (InvalidDataException ex)
                    {
                        AppLogger.Error($"[SERVER] Malformed payload from {remoteIp}: " + ex.Message);
                    }
                    catch (Exception ex)
                    {
                        AppLogger.Error($"[SERVER] ERROR handling print job from {remoteIp}: " + ex.Message);
                    }
                }
            }
        }
    }
}
