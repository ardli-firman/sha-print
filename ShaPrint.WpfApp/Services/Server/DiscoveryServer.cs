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
using ShaPrint.WpfApp.Services;
using ShaPrint.Core.Network;

namespace ShaPrint.Server
{
    public class DiscoveryServer
    {
        private UdpClient? _udpClient;
        private CancellationTokenSource? _cts;
        private List<string> _exposedPrinters = new List<string>();
        private List<string> _exposedScanners = new List<string>();
        private readonly INotificationService _notificationService;
        private string? _serverId;

        public void SetServerId(string? serverId) => _serverId = serverId;

        // Client tracking for connect/disconnect notifications
        private readonly HashSet<string> _connectedClients = new();
        private readonly ConcurrentDictionary<string, DateTime> _lastSeenByClient = new();
        private readonly ConcurrentDictionary<string, DateTime> _connectionStartTime = new();
        private readonly HashSet<string> _monitorClients = new();
        private int _requestCount;

        private readonly ScannerService _scannerService = new ScannerService();

        // Rate limiting: max 5 requests per second per IP
        private readonly ConcurrentDictionary<string, RateLimitEntry> _rateLimits = new();
        private const int MaxRequestsPerSecond = 5;
        private const int RateLimitWindowMs = 1000;

        private class RateLimitEntry
        {
            public int Count;
            public long WindowStart;
        }
        public DiscoveryServer(INotificationService notificationService)
        {
            _notificationService = notificationService;
        }

        public Dictionary<string, DateTime> GetActiveClientsWithConnectionTimes()
        {
            var result = new Dictionary<string, DateTime>();
            lock (_connectedClients)
            {
                foreach (var ip in _connectedClients)
                {
                    if (!_monitorClients.Contains(ip))
                    {
                        _connectionStartTime.TryGetValue(ip, out var startTime);
                        result[ip] = startTime == default ? DateTime.UtcNow : startTime;
                    }
                }
            }
            return result;
        }


        public void SetExposedPrinters(List<string> printers)
        {
            _exposedPrinters = printers;
        }

        public void SetExposedScanners(List<string> scanners)
        {
            _exposedScanners = scanners;
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

        /// <summary>
        /// Removes rate-limit entries whose window has expired, preventing unbounded memory growth.
        /// </summary>
        private void PruneStaleRateLimits()
        {
            long now = Environment.TickCount64;
            long cutoff = now - RateLimitWindowMs - 5000; // 5s grace beyond window
            foreach (var kvp in _rateLimits)
            {
                if (kvp.Value.WindowStart < cutoff)
                    _rateLimits.TryRemove(kvp.Key, out _);
            }
        }
        private async Task ListenLoopAsync(CancellationToken token)
        {
            int requestCount = 0;
            while (!token.IsCancellationRequested)
            {
                try
                {
                    var result = await _udpClient!.ReceiveAsync(token);
                    string request = Encoding.UTF8.GetString(result.Buffer);
                    string remoteIp = result.RemoteEndPoint.Address.ToString();

                    bool isMonitorRequest = (request == Constants.MonitorDiscoveryRequestMessage);
                    if (request != Constants.DiscoveryRequestMessage && !isMonitorRequest)
                        continue;

                    if (IsRateLimited(remoteIp))
                    {
                        AppLogger.Log($"[DISCOVERY] Rate limit hit from {remoteIp} — request dropped.");
                        continue;
                    }

                    // Periodic cleanup of stale rate-limit entries (every ~100 valid requests)
                    if (++requestCount % 100 == 0)
                        PruneStaleRateLimits();

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
                        ExposedPrinters = exposedInfos,
                        ExposedScanners = _exposedScanners.Count > 0 ? new List<ScannerInfo>() : null,
                        ServerId = _serverId
                    };

                    if (response.ExposedScanners != null)
                    {
                        try
                        {
                            var allLocalScanners = _scannerService.GetLocalScanners();
                            foreach (var s in _exposedScanners)
                            {
                                var found = allLocalScanners.FirstOrDefault(x => x.Name == s);
                                response.ExposedScanners.Add(new ScannerInfo
                                {
                                    Name = s,
                                    Description = found?.Description ?? "WIA Scanner"
                                });
                            }
                        }
                        catch (Exception ex)
                        {
                            AppLogger.Error("[DISCOVERY] Error loading exposed scanners detail", ex);
                        }
                    }

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


                    // Track client for connect/disconnect notifications
                    bool isNewClient;
                    lock (_connectedClients)
                    {
                        if (isMonitorRequest)
                        {
                            _monitorClients.Add(remoteIp);
                        }
                        isNewClient = _connectedClients.Add(remoteIp);
                        if (isNewClient)
                        {
                            _connectionStartTime[remoteIp] = DateTime.UtcNow;
                        }
                    }

                    if (isNewClient && !isMonitorRequest)
                    {
                        _notificationService.ShowClientConnected(remoteIp);
                    }
                    _lastSeenByClient[remoteIp] = DateTime.UtcNow;

                    // Periodic cleanup of disconnected clients (every ~50 requests)
                    if (++_requestCount % 50 == 0)
                    {
                        var cutoff = DateTime.UtcNow.AddMinutes(-5);
                        foreach (var kvp in _lastSeenByClient)
                        {
                            if (kvp.Value < cutoff)
                            {
                                var disconnectedIp = kvp.Key;
                                bool isMonitor;
                                lock (_connectedClients)
                                {
                                    _connectedClients.Remove(disconnectedIp);
                                    _connectionStartTime.TryRemove(disconnectedIp, out _);
                                    isMonitor = _monitorClients.Remove(disconnectedIp);
                                }
                                _lastSeenByClient.TryRemove(disconnectedIp, out _);
                                if (!isMonitor)
                                {
                                    _notificationService.ShowClientDisconnected(disconnectedIp);
                                }
                            }
                        }
                    }
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
