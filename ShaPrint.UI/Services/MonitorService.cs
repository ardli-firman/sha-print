using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.UI.Services;

/// <summary>Raised with the servers discovered in one UDP cycle (replaces
/// <c>MonitorViewModel.RegisterDiscoveredServers</c>).</summary>
public sealed class MonitorServersDiscoveredEventArgs
{
    public IReadOnlyList<DiscoveryResponseMessage> Servers { get; }

    public MonitorServersDiscoveredEventArgs(IReadOnlyList<DiscoveryResponseMessage> servers)
    {
        Servers = servers;
    }
}

/// <summary>Raised after a poll cycle completes so the UI can flag servers that were NOT
/// rediscovered (offline) and update the last-refresh timestamp.</summary>
public sealed class MonitorServersReconciledEventArgs
{
    public IReadOnlyList<DiscoveryResponseMessage> Servers { get; }
    public DateTime LastRefreshTime { get; }

    public MonitorServersReconciledEventArgs(IReadOnlyList<DiscoveryResponseMessage> servers, DateTime lastRefreshTime)
    {
        Servers = servers;
        LastRefreshTime = lastRefreshTime;
    }
}

/// <summary>Raised when the MonitorTcpServer (port 9878) responded with a fresh
/// <see cref="ServerStatusPayload"/>.</summary>
public sealed class MonitorServerStatusUpdatedEventArgs
{
    /// <summary>Shape from <see cref="ShaPrint.Core.Network"/> — the SOURCE OF TRUTH for the
    /// monitor bindings (HostName, Version, NetworkChannel, UptimeSeconds, Printers, ActiveClients).</summary>
    public ServerStatusPayload Payload { get; }
    public string IpAddress { get; }

    public MonitorServerStatusUpdatedEventArgs(ServerStatusPayload payload, string ipAddress)
    {
        Payload = payload;
        IpAddress = ipAddress;
    }
}

/// <summary>Raised when a server query failed (auth mismatch / unreachable / protocol error).</summary>
public sealed class MonitorServerFailureEventArgs
{
    public string ServerName { get; }
    public string IpAddress { get; }
    public string Reason { get; }

    public MonitorServerFailureEventArgs(string serverName, string ipAddress, string reason)
    {
        ServerName = serverName;
        IpAddress = ipAddress;
        Reason = reason;
    }
}

/// <summary>
/// Shared monitor polling service. Migrated from
/// <c>ShaPrint.WpfApp/Services/Monitor/MonitorService.cs</c> (Task 4) as an injectable class.
/// The WPF <c>MonitorViewModel</c> dependency is replaced by events, so the service stays UI-agnostic
/// and mockable; the Avalonia MonitorViewModel (Task 5) subscribes to these events.
///
/// Status payload is <see cref="ServerStatusPayload"/> from <c>ShaPrint.Core.Network</c>
/// (not a monitor-specific type).
/// </summary>
public class MonitorService
{
    private readonly DiscoveryClientService _discoveryClient;
    private CancellationTokenSource? _cts;
    private int _isRefreshing = 0;
    private readonly List<Task> _inFlightTasks = new();
    private readonly object _inFlightLock = new();

    public event EventHandler<MonitorServersDiscoveredEventArgs>? ServersDiscovered;
    public event EventHandler<MonitorServersReconciledEventArgs>? ServersReconciled;
    public event EventHandler<MonitorServerStatusUpdatedEventArgs>? ServerStatusUpdated;
    public event EventHandler<MonitorServerFailureEventArgs>? ServerFailure;

    /// <summary>Last completed poll cycle timestamp (UTC), set alongside <see cref="ServersReconciled"/>.</summary>
    public DateTime? LastRefreshTime { get; private set; }

    public MonitorService(DiscoveryClientService discoveryClient)
    {
        _discoveryClient = discoveryClient;
    }

    public void Start()
    {
        if (_cts != null) return;
        _cts = new CancellationTokenSource();
        Task.Run(() => PollLoopAsync(_cts.Token));
        AppLogger.Log("[MONITOR SERVICE] Service started.");
    }

    public void Stop()
    {
        if (_cts == null) return;
        _cts.Cancel();
        _cts = null;

        Task? waitTask;
        lock (_inFlightLock)
        {
            waitTask = _inFlightTasks.Count > 0
                ? Task.WhenAll(_inFlightTasks.ToArray())
                : null;
        }
        if (waitTask != null)
        {
            try { waitTask.Wait(TimeSpan.FromSeconds(10)); }
            catch (AggregateException) { /* expected */ }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR SERVICE] Error awaiting in-flight tasks during stop", ex);
            }
        }

        AppLogger.Log("[MONITOR SERVICE] Service stopped.");
    }

    public async Task TriggerManualRefreshAsync()
    {
        if (Interlocked.CompareExchange(ref _isRefreshing, 1, 0) == 1) return;
        AppLogger.Log("[MONITOR SERVICE] Triggering manual full refresh (with unicast sweep)...");
        try
        {
            var cts = _cts;
            var token = cts?.Token ?? CancellationToken.None;

            // Unicast sweep allowed on manual refresh to find AP isolated servers
            var discovered = await _discoveryClient.DiscoverServersAsync(
                skipUnicastSweep: false,
                requestMessage: Constants.MonitorDiscoveryRequestMessage);

            await QueryAllServersStaggeredAsync(discovered, token);
        }
        catch (Exception ex)
        {
            AppLogger.Error("[MONITOR SERVICE] Manual refresh failed", ex);
        }
        finally
        {
            Interlocked.Exchange(ref _isRefreshing, 0);
        }
    }

    private async Task PollLoopAsync(CancellationToken token)
    {
        // --- Wait for any ongoing manual refresh to complete ---
        while (Interlocked.CompareExchange(ref _isRefreshing, 0, 0) == 1 && !token.IsCancellationRequested)
        {
            try { await Task.Delay(500, token); } catch (OperationCanceledException) { return; }
        }

        // Initial poll at startup
        try
        {
            if (Interlocked.CompareExchange(ref _isRefreshing, 1, 0) == 0)
            {
                var initialDiscovered = await _discoveryClient.DiscoverServersAsync(
                    skipUnicastSweep: false,
                    requestMessage: Constants.MonitorDiscoveryRequestMessage);
                await QueryAllServersStaggeredAsync(initialDiscovered, token);
            }
        }
        catch (OperationCanceledException) { /* Graceful shutdown */ }
        catch (Exception ex)
        {
            AppLogger.Error("[MONITOR SERVICE] Initial discovery failed", ex);
        }
        finally
        {
            Interlocked.Exchange(ref _isRefreshing, 0);
        }

        while (!token.IsCancellationRequested)
        {
            try
            {
                await Task.Delay(TimeSpan.FromSeconds(15), token);

                if (Interlocked.CompareExchange(ref _isRefreshing, 1, 0) == 1) continue;

                try
                {
                    var discovered = await _discoveryClient.DiscoverServersAsync(
                        skipUnicastSweep: true,
                        requestMessage: Constants.MonitorDiscoveryRequestMessage);

                    await QueryAllServersStaggeredAsync(discovered, token);
                }
                finally
                {
                    Interlocked.Exchange(ref _isRefreshing, 0);
                }
            }
            catch (OperationCanceledException) { break; }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR SERVICE] Error in polling loop", ex);
            }
        }
    }

    private async Task QueryAllServersStaggeredAsync(List<DiscoveryResponseMessage> discoveredServers, CancellationToken token)
    {
        // Ensure all discovered servers exist in the UI collection
        ServersDiscovered?.Invoke(this, new MonitorServersDiscoveredEventArgs(discoveredServers));

        foreach (var server in discoveredServers)
        {
            Task queryTask;
            lock (_inFlightLock)
            {
                if (token.IsCancellationRequested) break;

                queryTask = QueryServerStatusAsync(server.ServerName, server.IpAddress, token);
                _inFlightTasks.Add(queryTask);
            }

            _ = queryTask.ContinueWith(t =>
            {
                lock (_inFlightLock) _inFlightTasks.Remove(queryTask);
            }, TaskContinuationOptions.ExecuteSynchronously);

            // Stagger requests by 1 second to avoid network and CPU spikes
            try
            {
                await Task.Delay(TimeSpan.FromSeconds(1), token);
            }
            catch (OperationCanceledException) { break; }
        }

        // Flag offline servers that were NOT in the discovered list
        LastRefreshTime = DateTime.UtcNow;
        ServersReconciled?.Invoke(this, new MonitorServersReconciledEventArgs(discoveredServers, LastRefreshTime.Value));
    }

    private async Task QueryServerStatusAsync(string hostName, string ipAddress, CancellationToken token)
    {
        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(token);
        timeoutCts.CancelAfter(TimeSpan.FromSeconds(5)); // 5 second TCP timeout

        try
        {
            using var tcpClient = new TcpClient();
            await tcpClient.ConnectAsync(ipAddress, Constants.MonitorTcpPort, timeoutCts.Token);

            using var stream = tcpClient.GetStream();
            stream.ReadTimeout = 5000;
            stream.WriteTimeout = 5000;

            // Write GET_STATUS request encrypted
            byte[] requestBytes = Encoding.UTF8.GetBytes("GET_STATUS");
            byte[] encryptedRequest = CryptoHelper.EncryptAesGcm(requestBytes);

            var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
            writer.Write(encryptedRequest.Length);
            writer.Write(encryptedRequest);
            writer.Flush();

            // Read encrypted response
            var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);
            int encryptedLength = reader.ReadInt32();
            if (encryptedLength < 0 || encryptedLength > 1024 * 1024) // limit to 1MB
            {
                throw new InvalidDataException($"Response payload size out of range: {encryptedLength}");
            }

            byte[] encryptedResponse = reader.ReadBytes(encryptedLength);
            if (encryptedResponse.Length != encryptedLength)
            {
                throw new InvalidDataException("Truncated response payload received.");
            }

            byte[] decryptedBytes = CryptoHelper.DecryptAesGcm(encryptedResponse);
            string json = Encoding.UTF8.GetString(decryptedBytes);

            var payload = JsonSerializer.Deserialize<ServerStatusPayload>(json, new JsonSerializerOptions
            {
                PropertyNameCaseInsensitive = true
            });

            if (payload != null)
            {
                payload.HostName = hostName; // Normalise hostname
                ServerStatusUpdated?.Invoke(this, new MonitorServerStatusUpdatedEventArgs(payload, ipAddress));
            }
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested)
        {
            // Graceful cancellation on service stop, ignore
        }
        catch (System.Security.Cryptography.CryptographicException ex)
        {
            AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) auth mismatch: {ex.Message}");
            ServerFailure?.Invoke(this, new MonitorServerFailureEventArgs(hostName, ipAddress, "AuthMismatch"));
        }
        catch (Exception ex) when (ex is IOException || ex is SocketException || ex is TimeoutException)
        {
            AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) is unreachable: {ex.Message}");
            ServerFailure?.Invoke(this, new MonitorServerFailureEventArgs(hostName, ipAddress, "Unreachable"));
        }
        catch (Exception ex) when (ex is JsonException || ex is InvalidDataException)
        {
            AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) protocol error: {ex.Message}");
            ServerFailure?.Invoke(this, new MonitorServerFailureEventArgs(hostName, ipAddress, "Unreachable"));
        }
        catch (Exception ex)
        {
            AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) unexpected error: {ex.Message}");
            ServerFailure?.Invoke(this, new MonitorServerFailureEventArgs(hostName, ipAddress, "Unreachable"));
        }
    }
}