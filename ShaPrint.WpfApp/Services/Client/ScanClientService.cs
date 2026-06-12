using System;
using System.IO;
using System.Net.Sockets;
using System.Text;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Client
{
    public class ScanClientService
    {
        public async Task<ScanResponsePayload> RequestScanAsync(string serverIp, string scannerName, int dpi, int colorMode, string format)
        {
            try
            {
                AppLogger.Log($"[CLIENT] Connecting to scanner server {serverIp}:{Constants.PrintTcpPort}...");
                using var client = new TcpClient();
                
                // Connect with a timeout of 10 seconds
                var connectTask = client.ConnectAsync(serverIp, Constants.PrintTcpPort);
                var delayTask = Task.Delay(TimeSpan.FromSeconds(10));
                
                if (await Task.WhenAny(connectTask, delayTask) == delayTask)
                {
                    throw new TimeoutException("Connection to scanner server timed out.");
                }
                
                await connectTask; // Propagate any connection exception

                using var stream = client.GetStream();
                
                // Step 1: Write multiplexing packet header
                var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
                writer.Write(Constants.PacketTypeScan); // 0x00000002
                writer.Flush();

                // Step 2: Write Scan Request Payload
                var request = new ScanRequestPayload
                {
                    TargetScannerName = scannerName,
                    Dpi = dpi,
                    ColorMode = colorMode,
                    Format = format
                };

                AppLogger.Log($"[CLIENT] Sending scan request to {serverIp}: scanner='{scannerName}', DPI={dpi}, Mode={colorMode}, Format={format}");
                await ScanRequestPayload.WriteAsync(stream, request);

                // Step 3: Read Scan Response Payload
                AppLogger.Log("[CLIENT] Waiting for scan results (this might take several seconds depending on scanner speed)...");
                var response = await ScanResponsePayload.ReadAsync(stream);

                return response;
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
