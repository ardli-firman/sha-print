using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;

namespace ShaPrint.WpfApp.Services.Server
{
    public class MonitorTcpServer
    {
        private readonly ServerStatusProvider _statusProvider;
        private TcpListener? _listener;
        private CancellationTokenSource? _cts;

        public MonitorTcpServer(ServerStatusProvider statusProvider)
        {
            _statusProvider = statusProvider;
        }

        public void Start()
        {
            _cts = new CancellationTokenSource();
            _listener = new TcpListener(IPAddress.Any, Constants.MonitorTcpPort);
            _listener.Start();
            Task.Run(() => AcceptLoopAsync(_cts.Token));
            AppLogger.Log($"[MONITOR SERVER] Started listening on port {Constants.MonitorTcpPort}");
        }

        public void Stop()
        {
            _cts?.Cancel();
            try { _listener?.Stop(); } catch { }
            AppLogger.Log("[MONITOR SERVER] Stopped listening");
        }

        private async Task AcceptLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    var client = await _listener!.AcceptTcpClientAsync(token);
                    _ = HandleClientAsync(client, token);
                }
                catch (OperationCanceledException) { break; }
                catch (ObjectDisposedException) { break; }
                catch (SocketException ex)
                {
                    AppLogger.Error("[MONITOR SERVER] Socket error in accept loop", ex);
                }
                catch (Exception ex)
                {
                    AppLogger.Error("[MONITOR SERVER] Unexpected error in accept loop", ex);
                }
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

                using (var stream = client.GetStream())
                {
                    try
                    {
                        var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);
                        int encryptedLength = reader.ReadInt32();
                        if (encryptedLength < 0 || encryptedLength > 4096)
                        {
                            throw new InvalidDataException($"Invalid encrypted status request length: {encryptedLength}");
                        }

                        byte[] encryptedBlob = reader.ReadBytes(encryptedLength);
                        if (encryptedBlob.Length != encryptedLength)
                        {
                            throw new InvalidDataException("Truncated request payload.");
                        }

                        byte[] decrypted = CryptoHelper.DecryptAesGcm(encryptedBlob);
                        string command = Encoding.UTF8.GetString(decrypted);

                        if (command == "GET_STATUS")
                        {
                            var status = _statusProvider.BuildStatus();
                            string json = JsonSerializer.Serialize(status);
                            byte[] jsonBytes = Encoding.UTF8.GetBytes(json);
                            byte[] encryptedResponse = CryptoHelper.EncryptAesGcm(jsonBytes);

                            var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
                            writer.Write(encryptedResponse.Length);
                            writer.Write(encryptedResponse);
                            writer.Flush();
                        }
                        else
                        {
                            AppLogger.Log($"[MONITOR SERVER] Unknown command from {remoteIp}: '{command}'");
                        }
                    }
                    catch (Exception ex)
                    {
                        AppLogger.Error($"[MONITOR SERVER] Error handling client {remoteIp}: {ex.Message}");
                    }
                }
            }
        }
    }
}
