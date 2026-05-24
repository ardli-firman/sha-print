using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Server
{
    public class DiscoveryServer
    {
        private UdpClient? _udpClient;
        private CancellationTokenSource? _cts;
        private List<string> _exposedPrinters = new List<string>();

        // Rate limiting: max 5 requests per second per IP
        private readonly ConcurrentDictionary<string, RateLimitEntry> _rateLimits = new();
        private const int MaxRequestsPerSecond = 5;
        private const int RateLimitWindowMs = 1000;

        private class RateLimitEntry
        {
            public int Count;
            public long WindowStart;
        }

        public void SetExposedPrinters(List<string> printers)
        {
            _exposedPrinters = printers;
        }

        public void Start()
        {
            _cts = new CancellationTokenSource();
            _udpClient = new UdpClient(Constants.DiscoveryUdpPort);
            Task.Run(() => ListenLoopAsync(_cts.Token));
        }

        public void Stop()
        {
            _cts?.Cancel();
            try { _udpClient?.Close(); } catch (Exception ex) { AppLogger.Error("[DISCOVERY] Error closing UDP client", ex); }
        }

        private bool IsRateLimited(string ip)
        {
            long now = Environment.TickCount64;
            var entry = _rateLimits.GetOrAdd(ip, _ => new RateLimitEntry { WindowStart = now, Count = 0 });

            if (now - entry.WindowStart > RateLimitWindowMs)
            {
                entry.WindowStart = now;
                entry.Count = 0;
            }

            entry.Count++;
            return entry.Count > MaxRequestsPerSecond;
        }

        private async Task ListenLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    var result = await _udpClient!.ReceiveAsync(token);
                    string request = Encoding.UTF8.GetString(result.Buffer);
                    string remoteIp = result.RemoteEndPoint.Address.ToString();

                    if (request != Constants.DiscoveryRequestMessage)
                        continue;

                    if (IsRateLimited(remoteIp))
                    {
                        AppLogger.Log($"[DISCOVERY] Rate limit hit from {remoteIp} — request dropped.");
                        continue;
                    }

                    var allDetailedPrinters = SpoolerApi.GetLocalPrintersDetailed();
                    var exposedInfos = new List<PrinterInfo>();
                    foreach (var p in _exposedPrinters)
                    {
                        var detailed = allDetailedPrinters.FirstOrDefault(x => x.Name == p);
                        exposedInfos.Add(new PrinterInfo 
                        { 
                            Name = p, 
                            Description = "Shared via ShaPrint",
                            DriverName = detailed?.DriverName ?? "Generic / Text Only"
                        });
                    }

                    var response = new DiscoveryResponseMessage
                    {
                        ServerName = Environment.MachineName,
                        IpAddress = GetLocalIPAddress(),
                        ExposedPrinters = exposedInfos
                    };

                    // Enforce response size limit
                    string jsonResponse = JsonSerializer.Serialize(response);
                    if (jsonResponse.Length > Constants.MaxDiscoveryResponseBytes)
                    {
                        AppLogger.Log($"[DISCOVERY] Response too large ({jsonResponse.Length} bytes), truncating printer list.");
                        while (exposedInfos.Count > 1 && jsonResponse.Length > Constants.MaxDiscoveryResponseBytes)
                        {
                            exposedInfos.RemoveAt(exposedInfos.Count - 1);
                            response.ExposedPrinters = exposedInfos;
                            jsonResponse = JsonSerializer.Serialize(response);
                        }
                    }

                    // Sign with HMAC so clients can verify authenticity
                    response.HmacSignature = CryptoHelper.SignHmac(Encoding.UTF8.GetBytes(jsonResponse));
                    jsonResponse = JsonSerializer.Serialize(response);

                    byte[] responseData = Encoding.UTF8.GetBytes(jsonResponse);
                    await _udpClient.SendAsync(responseData, responseData.Length, result.RemoteEndPoint);
                }
                catch (OperationCanceledException) { break; }
                catch (ObjectDisposedException) { break; }
                catch (SocketException ex)
                {
                    AppLogger.Error("[DISCOVERY] Socket error", ex);
                }
                catch (Exception ex)
                {
                    AppLogger.Error("[DISCOVERY] Unexpected error", ex);
                }
            }
        }

        private string GetLocalIPAddress()
        {
            try
            {
                var host = Dns.GetHostEntry(Dns.GetHostName());
                foreach (var ip in host.AddressList)
                {
                    if (ip.AddressFamily == AddressFamily.InterNetwork)
                        return ip.ToString();
                }
            }
            catch (Exception ex) { AppLogger.Error("[DISCOVERY] GetLocalIPAddress failed", ex); }
            return "127.0.0.1";
        }
    }
}
