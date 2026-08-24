using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Server;
using ShaPrint.WpfApp.Services;

namespace ShaPrint.Server;

public class DiscoveryServer
{
    private const int MaxRequestsPerSecond = 5;
    private const int MaxRateLimitEntries = 256;
    private const int MaxConcurrentRequests = 4;
    private const int MaxExposedDevices = 64;
    private const int MaxRequestBytes = 256;
    private static readonly TimeSpan RateLimitWindow = TimeSpan.FromSeconds(1);
    private static readonly TimeSpan DeviceQueryTimeout = TimeSpan.FromSeconds(5);

    private readonly INotificationService _notificationService;
    private readonly ScannerService _scannerService = new();
    private readonly object _lifecycleLock = new();
    private readonly SemaphoreSlim _requestSlots = new(MaxConcurrentRequests, MaxConcurrentRequests);
    private readonly ConcurrentDictionary<int, Task> _requestTasks = new();
    private readonly ConcurrentDictionary<string, RateLimitEntry> _rateLimits = new(StringComparer.Ordinal);
    private readonly HashSet<string> _connectedClients = new(StringComparer.OrdinalIgnoreCase);
    private readonly ConcurrentDictionary<string, DateTime> _lastSeenByClient = new(StringComparer.OrdinalIgnoreCase);
    private readonly ConcurrentDictionary<string, DateTime> _connectionStartTime = new(StringComparer.OrdinalIgnoreCase);

    private UdpClient? _udpClient;
    private CancellationTokenSource? _cts;
    private Task? _listenerTask;
    private string[] _exposedPrinters = Array.Empty<string>();
    private string[] _exposedScanners = Array.Empty<string>();
    private volatile string? _serverId;
    private DriverPackageService? _driverPackageService;
    private volatile bool _driverSharingEnabled = true;
    private int _nextRequestId;
    private int _requestCount;

    private sealed class RateLimitEntry
    {
        public int Count;
        public long WindowStart;
    }

    public DiscoveryServer(INotificationService notificationService)
    {
        _notificationService = notificationService;
    }

    public void SetServerId(string? serverId) => _serverId = serverId;

    public void SetDriverPackageService(DriverPackageService service)
        => _driverPackageService = service;

    public void SetDriverSharingEnabled(bool enabled)
        => _driverSharingEnabled = enabled;

    public void SetExposedPrinters(List<string> printers)
        => Volatile.Write(ref _exposedPrinters, SnapshotNames(printers));

    public void SetExposedScanners(List<string> scanners)
        => Volatile.Write(ref _exposedScanners, SnapshotNames(scanners));

    public Dictionary<string, DateTime> GetActiveClientsWithConnectionTimes()
    {
        var result = new Dictionary<string, DateTime>(StringComparer.OrdinalIgnoreCase);
        lock (_connectedClients)
        {
            foreach (string ip in _connectedClients)
            {
                _connectionStartTime.TryGetValue(ip, out DateTime startTime);
                result[ip] = startTime == default ? DateTime.UtcNow : startTime;
            }
        }
        return result;
    }

    public void Start()
    {
        lock (_lifecycleLock)
        {
            if (_cts != null)
                return;

            var cts = new CancellationTokenSource();
            var udpClient = new UdpClient(Constants.DiscoveryUdpPort);
            _cts = cts;
            _udpClient = udpClient;
            _listenerTask = ListenLoopAsync(udpClient, cts.Token);
        }
    }

    public void Stop() => StopAsync().GetAwaiter().GetResult();

    public async Task StopAsync()
    {
        CancellationTokenSource? cts;
        UdpClient? udpClient;
        Task? listenerTask;
        lock (_lifecycleLock)
        {
            cts = _cts;
            udpClient = _udpClient;
            listenerTask = _listenerTask;
            if (cts == null)
                return;
            cts.Cancel();
            udpClient?.Close();
        }

        try
        {
            if (listenerTask != null)
                await listenerTask.ConfigureAwait(false);

            Task[] handlers = _requestTasks.Values.ToArray();
            if (handlers.Length > 0)
                await Task.WhenAll(handlers).ConfigureAwait(false);
        }
        catch (OperationCanceledException) { }
        finally
        {
            lock (_lifecycleLock)
            {
                if (ReferenceEquals(_cts, cts))
                {
                    _cts = null;
                    _udpClient = null;
                    _listenerTask = null;
                }
            }
            udpClient?.Dispose();
            cts.Dispose();
        }
    }

    private async Task ListenLoopAsync(UdpClient udpClient, CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            try
            {
                UdpReceiveResult result = await udpClient.ReceiveAsync(token).ConfigureAwait(false);
                if (result.Buffer.Length == 0 || result.Buffer.Length > MaxRequestBytes)
                    continue;

                string request = Encoding.UTF8.GetString(result.Buffer);
                bool isMonitorRequest = request == Constants.MonitorDiscoveryRequestMessage;
                if (request != Constants.DiscoveryRequestMessage && !isMonitorRequest)
                    continue;

                string remoteIp = result.RemoteEndPoint.Address.ToString();
                if (IsRateLimited(remoteIp))
                    continue;
                if (!await _requestSlots.WaitAsync(0, token).ConfigureAwait(false))
                    continue;

                int requestId = Interlocked.Increment(ref _nextRequestId);
                Task requestTask = ProcessRequestWithCleanupAsync(
                    requestId,
                    udpClient,
                    result.RemoteEndPoint,
                    remoteIp,
                    isMonitorRequest,
                    token);
                _requestTasks[requestId] = requestTask;
                _ = requestTask.ContinueWith(
                    (_, state) =>
                    {
                        var tuple = ((ConcurrentDictionary<int, Task> Tasks, int Id))state!;
                        tuple.Tasks.TryRemove(tuple.Id, out Task? _);
                    },
                    (_requestTasks, requestId),
                    CancellationToken.None,
                    TaskContinuationOptions.ExecuteSynchronously,
                    TaskScheduler.Default);
            }
            catch (OperationCanceledException) when (token.IsCancellationRequested) { break; }
            catch (ObjectDisposedException) when (token.IsCancellationRequested) { break; }
            catch (SocketException ex) when (token.IsCancellationRequested || ex.SocketErrorCode == SocketError.OperationAborted) { break; }
            catch (Exception ex)
            {
                AppLogger.Error("[DISCOVERY] Listener error", ex);
            }
        }
    }

    private async Task ProcessRequestWithCleanupAsync(
        int requestId,
        UdpClient udpClient,
        IPEndPoint remoteEndPoint,
        string remoteIp,
        bool isMonitorRequest,
        CancellationToken token)
    {
        try
        {
            string[] printerSnapshot = Volatile.Read(ref _exposedPrinters);
            string[] scannerSnapshot = Volatile.Read(ref _exposedScanners);
            byte[]? responseBytes = await BuildResponseAsync(
                printerSnapshot,
                scannerSnapshot,
                isMonitorRequest,
                token).ConfigureAwait(false);
            if (responseBytes == null)
                return;

            // Commit client accounting before publishing the response so callers that
            // immediately inspect connection state observe a consistent result.
            TrackClient(remoteIp, isMonitorRequest);
            try
            {
                await udpClient.SendAsync(responseBytes, remoteEndPoint, token).ConfigureAwait(false);
            }
            finally
            {
                CryptographicOperations.ZeroMemory(responseBytes);
            }
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { }
        catch (Exception ex)
        {
            AppLogger.Error($"[DISCOVERY] Request from {remoteIp} failed", ex);
        }
        finally
        {
            _requestSlots.Release();
        }
    }

    private async Task<byte[]?> BuildResponseAsync(
        string[] exposedPrinters,
        string[] exposedScanners,
        bool isMonitorRequest,
        CancellationToken token)
    {
        List<PrinterInfo> localPrinters;
        try
        {
            localPrinters = await Task.Run(SpoolerApi.GetLocalPrintersDetailed, CancellationToken.None)
                .WaitAsync(DeviceQueryTimeout, token)
                .ConfigureAwait(false);
        }
        catch (Exception ex) when (ex is TimeoutException || ex is not OperationCanceledException)
        {
            AppLogger.Log($"[DISCOVERY] Printer detail query unavailable: {ex.GetType().Name}.");
            localPrinters = new List<PrinterInfo>();
        }

        var printerInfos = new List<PrinterInfo>(exposedPrinters.Length);
        foreach (string printerName in exposedPrinters)
        {
            token.ThrowIfCancellationRequested();
            PrinterInfo? detailed = localPrinters.FirstOrDefault(
                item => string.Equals(item.Name, printerName, StringComparison.OrdinalIgnoreCase));
            string driverName = detailed?.DriverName ?? "Generic / Text Only";
            var printerInfo = new PrinterInfo
            {
                Name = printerName,
                Description = "Shared via ShaPrint",
                DriverName = driverName
            };

            if (!isMonitorRequest && _driverSharingEnabled && _driverPackageService != null)
            {
                try
                {
                    var package = await _driverPackageService.GetDriverPackageAsync(driverName, token)
                        .WaitAsync(DeviceQueryTimeout, token)
                        .ConfigureAwait(false);
                    if (package != null)
                    {
                        printerInfo.DriverAvailable = true;
                        printerInfo.DriverPackageId = package.Sha256;
                        printerInfo.DriverSizeBytes = package.TotalSizeBytes;
                    }
                }
                catch (Exception ex) when (ex is TimeoutException || ex is not OperationCanceledException)
                {
                    AppLogger.Log($"[DISCOVERY] Driver metadata unavailable for '{driverName}': {ex.GetType().Name}.");
                }
            }

            printerInfos.Add(printerInfo);
        }

        var scannerInfos = new List<ScannerInfo>();
        if (exposedScanners.Length > 0)
        {
            List<ScannerInfo> localScanners = await _scannerService
                .GetLocalScannersAsync(token, DeviceQueryTimeout)
                .ConfigureAwait(false);
            foreach (string scannerName in exposedScanners)
            {
                ScannerInfo? found = localScanners.FirstOrDefault(
                    item => string.Equals(item.Name, scannerName, StringComparison.OrdinalIgnoreCase));
                scannerInfos.Add(new ScannerInfo
                {
                    Name = scannerName,
                    Description = found?.Description ?? "WIA Scanner"
                });
            }
        }

        string localIp = GetLocalIPAddress();
        var response = new DiscoveryResponseMessage
        {
            ServerName = Environment.MachineName,
            IpAddress = localIp,
            ExposedPrinters = printerInfos,
            ExposedScanners = scannerInfos.Count == 0 ? null : scannerInfos,
            ServerId = _serverId,
            IppEndpoint = $"http://{localIp}:631/ipp/print"
        };

        while (true)
        {
            token.ThrowIfCancellationRequested();
            response.HmacSignature = null;
            byte[] unsignedBytes = JsonSerializer.SerializeToUtf8Bytes(response);
            try
            {
                response.HmacSignature = CryptoHelper.SignHmac(unsignedBytes);
            }
            finally
            {
                CryptographicOperations.ZeroMemory(unsignedBytes);
            }

            byte[] signedBytes = JsonSerializer.SerializeToUtf8Bytes(response);
            if (signedBytes.Length <= Constants.MaxDiscoveryResponseBytes)
                return signedBytes;

            CryptographicOperations.ZeroMemory(signedBytes);
            if (response.ExposedScanners?.Count > 0)
            {
                response.ExposedScanners.RemoveAt(response.ExposedScanners.Count - 1);
                if (response.ExposedScanners.Count == 0)
                    response.ExposedScanners = null;
                continue;
            }
            if (response.ExposedPrinters.Count > 0)
            {
                response.ExposedPrinters.RemoveAt(response.ExposedPrinters.Count - 1);
                continue;
            }

            AppLogger.Log("[DISCOVERY] Base response exceeds the protocol size limit; response dropped.");
            return null;
        }
    }

    private bool IsRateLimited(string ip)
    {
        long now = Environment.TickCount64;
        if (!_rateLimits.ContainsKey(ip) && _rateLimits.Count >= MaxRateLimitEntries)
            PruneStaleRateLimits(now);
        if (!_rateLimits.ContainsKey(ip) && _rateLimits.Count >= MaxRateLimitEntries)
            return true;

        RateLimitEntry entry = _rateLimits.GetOrAdd(ip, _ => new RateLimitEntry { WindowStart = now });
        lock (entry)
        {
            if (now - entry.WindowStart >= RateLimitWindow.TotalMilliseconds)
            {
                entry.WindowStart = now;
                entry.Count = 0;
            }
            return ++entry.Count > MaxRequestsPerSecond;
        }
    }

    private void PruneStaleRateLimits(long now)
    {
        foreach (var pair in _rateLimits)
        {
            if (now - pair.Value.WindowStart >= RateLimitWindow.TotalMilliseconds * 2)
                _rateLimits.TryRemove(pair.Key, out _);
        }
    }

    private void TrackClient(string remoteIp, bool isMonitorRequest)
    {
        if (isMonitorRequest)
            return;

        bool isNewClient;
        lock (_connectedClients)
        {
            isNewClient = _connectedClients.Add(remoteIp);
            if (isNewClient)
                _connectionStartTime[remoteIp] = DateTime.UtcNow;
        }
        _lastSeenByClient[remoteIp] = DateTime.UtcNow;
        if (isNewClient)
            _notificationService.ShowClientConnected(remoteIp);

        if (Interlocked.Increment(ref _requestCount) % 50 != 0)
            return;

        DateTime cutoff = DateTime.UtcNow.AddMinutes(-5);
        foreach (var pair in _lastSeenByClient)
        {
            if (pair.Value >= cutoff)
                continue;
            bool removed;
            lock (_connectedClients)
            {
                removed = _connectedClients.Remove(pair.Key);
                _connectionStartTime.TryRemove(pair.Key, out _);
            }
            _lastSeenByClient.TryRemove(pair.Key, out _);
            if (removed)
                _notificationService.ShowClientDisconnected(pair.Key);
        }
    }

    private static string[] SnapshotNames(IEnumerable<string> names)
        => names
            .Where(name => !string.IsNullOrWhiteSpace(name))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .Take(MaxExposedDevices)
            .ToArray();

    private static string GetLocalIPAddress()
    {
        try
        {
            return Dns.GetHostEntry(Dns.GetHostName()).AddressList
                .FirstOrDefault(ip => ip.AddressFamily == AddressFamily.InterNetwork)
                ?.ToString() ?? "127.0.0.1";
        }
        catch (Exception ex)
        {
            AppLogger.Log($"[DISCOVERY] Local IP lookup failed: {ex.GetType().Name}.");
            return "127.0.0.1";
        }
    }
}
