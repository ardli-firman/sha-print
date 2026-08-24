using System;
using System.IO;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.Services;

namespace ShaPrint.Client
{
    public class ScanClientService
    {
        public static readonly TimeSpan DefaultRequestTimeout = TimeSpan.FromMinutes(5);
        private readonly INotificationService _notificationService;

        public ScanClientService(INotificationService notificationService)
        {
            _notificationService = notificationService;
        }

        public async Task<ScanResponsePayload> RequestScanAsync(
            string serverIp,
            string scannerName,
            int dpi,
            int colorMode,
            string format,
            CancellationToken cancellationToken = default,
            TimeSpan? timeout = null)
        {
            using var requestTimeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            requestTimeout.CancelAfter(timeout ?? DefaultRequestTimeout);
            CancellationToken token = requestTimeout.Token;
            try
            {
                AppLogger.Log($"[CLIENT] Connecting to scanner server {serverIp}:{Constants.PrintTcpPort}...");
                using var client = new TcpClient();
                await client.ConnectAsync(serverIp, Constants.PrintTcpPort, token).ConfigureAwait(false);

                using var stream = client.GetStream();

                // Step 1: Write multiplexing packet header
                byte[] packetHeader = BitConverter.GetBytes(Constants.PacketTypeScan);
                await stream.WriteAsync(packetHeader, token).ConfigureAwait(false);
                await stream.FlushAsync(token).ConfigureAwait(false);

                // Step 2: Write Scan Request Payload
                var request = new ScanRequestPayload
                {
                    TargetScannerName = scannerName,
                    Dpi = dpi,
                    ColorMode = colorMode,
                    Format = format,
                    Brightness = 0,
                    Contrast = 0
                };

                AppLogger.Log($"[CLIENT] Sending scan request to {serverIp}: scanner='{scannerName}', DPI={dpi}, Mode={colorMode}, Format={format}");
                await ScanRequestPayload.WriteAsync(stream, request, token);

                // Step 3: Read Scan Response Payload
                AppLogger.Log("[CLIENT] Waiting for scan results (this might take several seconds depending on scanner speed)...");
                var response = await ScanResponsePayload.ReadAsync(stream, token);

                // Notify based on scan result
                string ext = format.ToLowerInvariant() switch
                {
                    "png" => "png",
                    "pdf" => "pdf",
                    _ => "jpg"
                };

                if (response.Success)
                {
                    _notificationService.ShowScanCompleted($"Scan_{DateTime.Now:yyyyMMdd_HHmmss}.{ext}");
                }
                else
                {
                    _notificationService.ShowScanFailed(response.ErrorMessage ?? "Unknown scan error");
                }

                return response;
            }
            catch (OperationCanceledException) when (requestTimeout.IsCancellationRequested)
            {
                const string message = "Scan request timed out or was cancelled.";
                AppLogger.Error($"[CLIENT] {message}");
                return new ScanResponsePayload
                {
                    Success = false,
                    ErrorMessage = message,
                    FileBytes = Array.Empty<byte>()
                };
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[CLIENT] Scan request failed: {ex.Message}", ex);
                return new ScanResponsePayload
                {
                    Success = false,
                    ErrorMessage = $"Scan connection failed: {ex.Message}",
                    FileBytes = Array.Empty<byte>()
                };
            }
        }
    }
}
