using System;
using System.Collections.Concurrent;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;

namespace ShaPrint.UI.Services
{
    /// <summary>
    /// TCP 9878 status server for monitor clients. Migrated verbatim from
    /// <c>ShaPrint.WpfApp/Services/Server/MonitorTcpServer.cs</c> (Gap-closing task) — the wire
    /// protocol is unchanged: length-prefixed AES-GCM blob carrying the plaintext command
    /// ("GET_STATUS"), response is <c>JsonSerializer.Serialize(ServerStatusPayload)</c>,
    /// AES-GCM encrypted, length-prefixed. Concurrency slot + per-IP rate limiting are preserved.
    /// Platform-agnostic (no WPF/System.Printing), so it also serves net8.0 (macOS/Linux) builds.
    /// </summary>
    public class MonitorTcpServer
    {
        private readonly ServerStatusProvider _statusProvider;
        private TcpListener? _listener;
        private CancellationTokenSource? _cts;
        private readonly SemaphoreSlim _concurrencySlot = new SemaphoreSlim(8, 8);
        private readonly ConcurrentDictionary<string, (int Count, DateTime WindowStart)> _rateLimiter = new();

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

                if (!await _concurrencySlot.WaitAsync(0, token))
                {
                    AppLogger.Log($"[MONITOR SERVER] Concurrency limit reached. Rejecting client {remoteIp}");
                    return;
                }

                try
                {
                    var now = DateTime.UtcNow;
                    _rateLimiter.AddOrUpdate(remoteIp,
                        _ => (1, now),
                        (_, entry) =>
                        {
                            if (now - entry.WindowStart > TimeSpan.FromSeconds(10))
                                return (1, now);
                            return (entry.Count + 1, entry.WindowStart);
                        });

                    if (_rateLimiter.TryGetValue(remoteIp, out var currentRate) && currentRate.Count > 6)
                    {
                        AppLogger.Log($"[MONITOR SERVER] Rate limit exceeded for IP {remoteIp}. Rejecting.");
                        return;
                    }

                    if (_rateLimiter.Count > 1000)
                    {
                        _rateLimiter.Clear();
                    }

                    using (var stream = client.GetStream())
                    {
                        stream.ReadTimeout = 5000;
                        stream.WriteTimeout = 5000;

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

                            byte[] decrypted;
                            try
                            {
                                decrypted = CryptoHelper.DecryptAesGcm(encryptedBlob);
                            }
                            catch (CryptographicException)
                            {
                                AppLogger.Log($"[MONITOR SERVER] Decrypt failed for {remoteIp}");
                                return;
                            }

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
                        catch (IOException)
                        {
                            AppLogger.Log($"[MONITOR SERVER] Stream I/O timeout or error for {remoteIp}");
                        }
                        catch (Exception ex)
                        {
                            AppLogger.Log($"[MONITOR SERVER] Error handling client {remoteIp}: {ex.GetType().Name}");
                        }
                    }
                }
                finally
                {
                    _concurrencySlot.Release();
                }
            }
        }
    }
}