using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Client;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.ViewModels.Pages;

namespace ShaPrint.WpfApp.Services.Monitor;

public sealed class MonitorService
{
    private const int MaxMonitoredServers = 32;
    private static readonly TimeSpan PollInterval = TimeSpan.FromSeconds(15);
    private static readonly TimeSpan QueryStagger = TimeSpan.FromMilliseconds(250);
    private static readonly TimeSpan RefreshDeadline = TimeSpan.FromSeconds(30);
    private static readonly TimeSpan RequestDeadline = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan StreamIdleDeadline = TimeSpan.FromSeconds(5);

    private readonly MonitorViewModel _monitorViewModel;
    private readonly DiscoveryClient _discoveryClient;
    private readonly SemaphoreSlim _refreshGate = new(1, 1);
    private readonly object _lifecycleLock = new();
    private readonly ConcurrentDictionary<int, Task> _activeRefreshes = new();
    private CancellationTokenSource? _cts;
    private Task? _pollTask;
    private Task? _stopTask;
    private bool _isStarted;
    private bool _isStopping;
    private int _nextRefreshId;

    public MonitorService(MonitorViewModel monitorViewModel)
        : this(monitorViewModel, new DiscoveryClient())
    {
    }

    internal MonitorService(MonitorViewModel monitorViewModel, DiscoveryClient discoveryClient)
    {
        _monitorViewModel = monitorViewModel;
        _discoveryClient = discoveryClient;
        _monitorViewModel.RefreshCommand = new AsyncRelayCommand(TriggerManualRefreshAsync);
    }

    public void Start()
    {
        lock (_lifecycleLock)
        {
            if (_isStarted || _isStopping)
                return;

            if (_cts == null || _cts.IsCancellationRequested)
                _cts = new CancellationTokenSource();
            _isStarted = true;
            _pollTask = Task.Run(() => PollLoopAsync(_cts.Token), CancellationToken.None);
        }
        AppLogger.Log("[MONITOR SERVICE] Service started.");
    }

    /// <summary>
    /// Compatibility wrapper for legacy callers. UI and shutdown paths should await
    /// <see cref="StopAsync"/> so they do not block their calling thread.
    /// </summary>
    public void Stop() => StopAsync().GetAwaiter().GetResult();

    public Task StopAsync()
    {
        lock (_lifecycleLock)
        {
            if (_stopTask != null)
                return _stopTask;
            if (_cts == null)
                return Task.CompletedTask;

            _isStopping = true;
            _cts.Cancel();
            _stopTask = StopCoreAsync(_cts, _pollTask);
            return _stopTask;
        }
    }

    private async Task StopCoreAsync(CancellationTokenSource cts, Task? pollTask)
    {
        try
        {
            if (pollTask != null)
            {
                try { await pollTask.ConfigureAwait(false); }
                catch (OperationCanceledException) { }
                catch (Exception ex) { AppLogger.Error("[MONITOR SERVICE] Poll shutdown failed", ex); }
            }

            Task[] activeRefreshes = _activeRefreshes.Values.ToArray();
            if (activeRefreshes.Length > 0)
            {
                try { await Task.WhenAll(activeRefreshes).ConfigureAwait(false); }
                catch (OperationCanceledException) { }
                catch (Exception ex) { AppLogger.Error("[MONITOR SERVICE] Refresh shutdown failed", ex); }
            }
        }
        finally
        {
            lock (_lifecycleLock)
            {
                if (ReferenceEquals(_cts, cts))
                {
                    _cts = null;
                    _pollTask = null;
                    _isStarted = false;
                    _isStopping = false;
                    _stopTask = null;
                }
            }
            cts.Dispose();
        }

        AppLogger.Log("[MONITOR SERVICE] Service stopped.");
    }

    public Task TriggerManualRefreshAsync()
    {
        CancellationToken token;
        lock (_lifecycleLock)
        {
            if (_isStopping)
                return Task.CompletedTask;
            if (_cts == null || _cts.IsCancellationRequested)
                _cts = new CancellationTokenSource();
            token = _cts.Token;
        }
        return TrackRefreshAsync(skipUnicastSweep: false, token);
    }

    private async Task PollLoopAsync(CancellationToken token)
    {
        await TrackRefreshAsync(skipUnicastSweep: false, token).ConfigureAwait(false);

        while (!token.IsCancellationRequested)
        {
            try
            {
                await Task.Delay(PollInterval, token).ConfigureAwait(false);
                await TrackRefreshAsync(skipUnicastSweep: true, token).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (token.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR SERVICE] Poll failed", ex);
            }
        }
    }

    private Task TrackRefreshAsync(bool skipUnicastSweep, CancellationToken token)
    {
        int refreshId = Interlocked.Increment(ref _nextRefreshId);
        Task task = RefreshAsync(skipUnicastSweep, token);
        _activeRefreshes[refreshId] = task;
        _ = task.ContinueWith(
            (_, state) =>
            {
                var tuple = ((ConcurrentDictionary<int, Task> Tasks, int Id))state!;
                tuple.Tasks.TryRemove(tuple.Id, out Task? _);
            },
            (_activeRefreshes, refreshId),
            CancellationToken.None,
            TaskContinuationOptions.ExecuteSynchronously,
            TaskScheduler.Default);
        return task;
    }

    private async Task RefreshAsync(bool skipUnicastSweep, CancellationToken cancellationToken)
    {
        if (!await _refreshGate.WaitAsync(0, cancellationToken).ConfigureAwait(false))
            return;

        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        deadline.CancelAfter(RefreshDeadline);
        try
        {
            List<DiscoveryResponseMessage> discovered = await _discoveryClient.DiscoverServersAsync(
                timeoutMs: 2000,
                skipUnicastSweep: skipUnicastSweep,
                requestMessage: Constants.MonitorDiscoveryRequestMessage,
                cancellationToken: deadline.Token).ConfigureAwait(false);

            await QueryAllServersStaggeredAsync(discovered, deadline.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            // Service shutdown: no UI state transition.
        }
        catch (OperationCanceledException)
        {
            AppLogger.Log("[MONITOR SERVICE] Refresh exceeded its total deadline.");
        }
        catch (Exception ex)
        {
            AppLogger.Error("[MONITOR SERVICE] Refresh failed", ex);
        }
        finally
        {
            _refreshGate.Release();
        }
    }

    private async Task QueryAllServersStaggeredAsync(
        List<DiscoveryResponseMessage> discoveredServers,
        CancellationToken token)
    {
        DiscoveryResponseMessage[] snapshot = discoveredServers
            .Where(server => !string.IsNullOrWhiteSpace(server.ServerName) &&
                             !string.IsNullOrWhiteSpace(server.IpAddress))
            .GroupBy(server => server.ServerName, StringComparer.OrdinalIgnoreCase)
            .Select(group => group.First())
            .Take(MaxMonitoredServers)
            .ToArray();

        var snapshotList = snapshot.ToList();
        _monitorViewModel.RegisterDiscoveredServers(snapshotList);

        var queries = new List<Task>(snapshot.Length);
        for (int index = 0; index < snapshot.Length; index++)
        {
            token.ThrowIfCancellationRequested();
            DiscoveryResponseMessage server = snapshot[index];
            queries.Add(QueryServerStatusAsync(server.ServerName, server.IpAddress, token));
            if (index + 1 < snapshot.Length)
                await Task.Delay(QueryStagger, token).ConfigureAwait(false);
        }

        if (queries.Count > 0)
            await Task.WhenAll(queries).ConfigureAwait(false);

        _monitorViewModel.FlagUndiscoveredServers(snapshotList);
        _monitorViewModel.LastRefreshTime = DateTime.UtcNow;
    }

    internal async Task QueryServerStatusAsync(string hostName, string ipAddress, CancellationToken serviceToken)
    {
        using var requestDeadline = CancellationTokenSource.CreateLinkedTokenSource(serviceToken);
        requestDeadline.CancelAfter(RequestDeadline);
        CancellationToken token = requestDeadline.Token;
        byte[]? requestBytes = null;
        byte[]? encryptedRequest = null;
        byte[]? encryptedResponse = null;
        byte[]? decryptedResponse = null;

        try
        {
            using var tcpClient = new TcpClient();
            await tcpClient.ConnectAsync(ipAddress, Constants.MonitorTcpPort, token).ConfigureAwait(false);
            using NetworkStream stream = tcpClient.GetStream();

            requestBytes = Encoding.UTF8.GetBytes("GET_STATUS");
            encryptedRequest = CryptoHelper.EncryptAesGcm(requestBytes);
            await MonitorFrameCodec.WriteAsync(
                stream,
                encryptedRequest,
                Constants.MaxMonitorRequestBytes,
                token,
                StreamIdleDeadline).ConfigureAwait(false);

            encryptedResponse = await MonitorFrameCodec.ReadAsync(
                stream,
                Constants.MaxMonitorResponseBytes,
                token,
                StreamIdleDeadline).ConfigureAwait(false);
            if (encryptedResponse.Length < Constants.AesGcmMinimumPayloadBytes)
                throw new InvalidDataException("Encrypted monitor response is too short.");

            decryptedResponse = CryptoHelper.DecryptAesGcm(encryptedResponse);
            var payload = JsonSerializer.Deserialize<ServerStatusPayload>(decryptedResponse, new JsonSerializerOptions
            {
                PropertyNameCaseInsensitive = true
            });
            if (payload == null)
                throw new InvalidDataException("Monitor response did not contain a status payload.");
            if (payload.Printers == null || payload.Scanners == null || payload.ActiveClients == null ||
                payload.RecentJobs == null || payload.Errors == null)
            {
                throw new InvalidDataException("Monitor response contains null collection fields.");
            }

            payload.HostName = hostName;
            _monitorViewModel.UpdateServerStatus(payload, ipAddress, isOnline: true);
        }
        catch (OperationCanceledException) when (serviceToken.IsCancellationRequested)
        {
            // Service stop: do not flip status.
        }
        catch (Exception ex)
        {
            MonitorFailureCategory category = MonitorFailureClassifier.Classify(ex);
            AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) failed: {category}.");
            _monitorViewModel.UpdateServerFailure(hostName, ipAddress, category.ToString());
        }
        finally
        {
            Zero(requestBytes);
            Zero(encryptedRequest);
            Zero(encryptedResponse);
            Zero(decryptedResponse);
        }
    }

    private static void Zero(byte[]? buffer)
    {
        if (buffer != null)
            CryptographicOperations.ZeroMemory(buffer);
    }
}
