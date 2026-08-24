using System;
using System.Collections.Concurrent;
using System.IO;
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

namespace ShaPrint.WpfApp.Services.Server;

public sealed class MonitorTcpServer : IDisposable
{
    private const int MaxConcurrentHandlers = 8;
    private const int MaxTrackedStatusWorkers = 8;
    private const int MaxRequestsPerWindow = 6;
    private const int MaxRateLimitEntries = 256;
    private static readonly TimeSpan RateLimitWindow = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan RequestDeadline = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan StreamIdleDeadline = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan OverloadWriteDeadline = TimeSpan.FromSeconds(1);
    private static readonly TimeSpan StopShutdownDeadline = TimeSpan.FromSeconds(2);

    private readonly Func<CancellationToken, ServerStatusPayload> _statusFactory;
    private readonly object _lifecycleLock = new();
    private readonly SemaphoreSlim _concurrencySlot = new(MaxConcurrentHandlers, MaxConcurrentHandlers);
    private readonly SemaphoreSlim _statusWorkerSlots = new(MaxTrackedStatusWorkers, MaxTrackedStatusWorkers);
    private readonly ConcurrentDictionary<int, Task> _handlerTasks = new();
    private readonly ConcurrentDictionary<int, Task> _statusWorkerTasks = new();
    private readonly ConcurrentDictionary<string, RateLimitEntry> _rateLimiter = new(StringComparer.Ordinal);
    private TcpListener? _listener;
    private CancellationTokenSource? _cts;
    private Task? _acceptTask;
    private int _nextHandlerId;
    private int _nextStatusWorkerId;
    private bool _disposed;

    private sealed class RateLimitEntry
    {
        public int Count;
        public long WindowStart;
    }

    public MonitorTcpServer(ServerStatusProvider statusProvider)
    {
        _statusFactory = token =>
        {
            token.ThrowIfCancellationRequested();
            ServerStatusPayload status = statusProvider.BuildStatus();
            token.ThrowIfCancellationRequested();
            return status;
        };
    }

    internal MonitorTcpServer(Func<CancellationToken, ServerStatusPayload> statusFactory)
    {
        _statusFactory = statusFactory;
    }

    public void Start()
    {
        lock (_lifecycleLock)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            if (_cts != null)
                return;

            var cts = new CancellationTokenSource();
            var listener = new TcpListener(IPAddress.Any, Constants.MonitorTcpPort);
            listener.Start(backlog: 16);
            _cts = cts;
            _listener = listener;
            _acceptTask = AcceptLoopAsync(listener, cts.Token);
        }
        AppLogger.Log($"[MONITOR SERVER] Listening on port {Constants.MonitorTcpPort}.");
    }

    public void Stop() => StopAsync().GetAwaiter().GetResult();

    public async Task StopAsync()
    {
        CancellationTokenSource? cts;
        TcpListener? listener;
        Task? acceptTask;
        lock (_lifecycleLock)
        {
            cts = _cts;
            listener = _listener;
            acceptTask = _acceptTask;
            if (cts == null)
                return;
            cts.Cancel();
            listener?.Stop();
        }

        try
        {
            using var shutdownDeadline = new CancellationTokenSource(StopShutdownDeadline);
            if (acceptTask != null)
            {
                try { await acceptTask.WaitAsync(shutdownDeadline.Token).ConfigureAwait(false); }
                catch (OperationCanceledException) { }
            }
            Task[] handlers = _handlerTasks.Values.ToArray();
            if (handlers.Length > 0)
            {
                try { await Task.WhenAll(handlers).WaitAsync(shutdownDeadline.Token).ConfigureAwait(false); }
                catch (OperationCanceledException) { }
                catch (Exception ex) { AppLogger.Log($"[MONITOR SERVER] Handler stopped with {ex.GetType().Name}."); }
            }

            Task[] workers = _statusWorkerTasks.Values.ToArray();
            if (workers.Length > 0)
            {
                try
                {
                    await Task.WhenAll(workers).WaitAsync(shutdownDeadline.Token).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    AppLogger.Log("[MONITOR SERVER] Status worker shutdown deadline reached; remaining workers stay tracked.");
                }
                catch (Exception ex)
                {
                    AppLogger.Log($"[MONITOR SERVER] Status worker completed with {ex.GetType().Name} during shutdown.");
                }
            }
        }
        catch (OperationCanceledException) { }
        finally
        {
            lock (_lifecycleLock)
            {
                if (ReferenceEquals(_cts, cts))
                {
                    _cts = null;
                    _listener = null;
                    _acceptTask = null;
                }
            }
            cts.Dispose();
            _rateLimiter.Clear();
        }

        AppLogger.Log("[MONITOR SERVER] Stopped.");
    }

    private async Task AcceptLoopAsync(TcpListener listener, CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            TcpClient? client = null;
            try
            {
                client = await listener.AcceptTcpClientAsync(token).ConfigureAwait(false);
                string remoteIp = GetRemoteIp(client);
                if (IsRateLimited(remoteIp) || !await _concurrencySlot.WaitAsync(0, token).ConfigureAwait(false))
                {
                    await RejectOverloadedAsync(client, token).ConfigureAwait(false);
                    client = null;
                    continue;
                }

                int handlerId = Interlocked.Increment(ref _nextHandlerId);
                Task handlerTask = HandleClientWithCleanupAsync(client, remoteIp, token);
                client = null;
                _handlerTasks[handlerId] = handlerTask;
                _ = handlerTask.ContinueWith(
                    (_, state) =>
                    {
                        var tuple = ((ConcurrentDictionary<int, Task> Tasks, int Id))state!;
                        tuple.Tasks.TryRemove(tuple.Id, out Task? _);
                    },
                    (_handlerTasks, handlerId),
                    CancellationToken.None,
                    TaskContinuationOptions.ExecuteSynchronously,
                    TaskScheduler.Default);
            }
            catch (OperationCanceledException) when (token.IsCancellationRequested) { break; }
            catch (ObjectDisposedException) when (token.IsCancellationRequested) { break; }
            catch (SocketException ex) when (token.IsCancellationRequested || ex.SocketErrorCode == SocketError.OperationAborted) { break; }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR SERVER] Accept loop error", ex);
            }
            finally
            {
                client?.Dispose();
            }
        }
    }

    private async Task RejectOverloadedAsync(TcpClient client, CancellationToken shutdownToken)
    {
        using (client)
        using (var deadline = CancellationTokenSource.CreateLinkedTokenSource(shutdownToken))
        {
            deadline.CancelAfter(OverloadWriteDeadline);
            try
            {
                await MonitorFrameCodec.WriteOverloadedAsync(
                    client.GetStream(),
                    deadline.Token,
                    OverloadWriteDeadline).ConfigureAwait(false);
            }
            catch (Exception ex) when (ex is IOException or SocketException or TimeoutException or OperationCanceledException)
            {
                // The peer may disconnect before reading the overload marker.
            }
        }
    }

    private async Task HandleClientWithCleanupAsync(
        TcpClient client,
        string remoteIp,
        CancellationToken shutdownToken)
    {
        using (client)
        using (var deadline = CancellationTokenSource.CreateLinkedTokenSource(shutdownToken))
        {
            deadline.CancelAfter(RequestDeadline);
            byte[]? encryptedRequest = null;
            byte[]? decryptedRequest = null;
            byte[]? jsonBytes = null;
            byte[]? encryptedResponse = null;
            try
            {
                NetworkStream stream = client.GetStream();
                encryptedRequest = await MonitorFrameCodec.ReadAsync(
                    stream,
                    Constants.MaxMonitorRequestBytes,
                    deadline.Token,
                    StreamIdleDeadline).ConfigureAwait(false);
                if (encryptedRequest.Length < Constants.AesGcmMinimumPayloadBytes)
                    throw new InvalidDataException("Encrypted monitor request is too short.");

                try
                {
                    decryptedRequest = CryptoHelper.DecryptAesGcm(encryptedRequest);
                }
                catch (CryptographicException)
                {
                    AppLogger.Log($"[MONITOR SERVER] Authentication failed for {remoteIp}.");
                    await MonitorFrameCodec.WriteAuthenticationFailedAsync(
                        stream,
                        deadline.Token,
                        StreamIdleDeadline).ConfigureAwait(false);
                    return;
                }

                if (!decryptedRequest.AsSpan().SequenceEqual("GET_STATUS"u8))
                {
                    AppLogger.Log($"[MONITOR SERVER] Unknown command from {remoteIp}.");
                    return;
                }

                Task<ServerStatusPayload>? statusWorker = StartStatusWorker(deadline.Token);
                if (statusWorker == null)
                {
                    await MonitorFrameCodec.WriteOverloadedAsync(
                        stream,
                        deadline.Token,
                        StreamIdleDeadline).ConfigureAwait(false);
                    return;
                }
                ServerStatusPayload status = await statusWorker.WaitAsync(deadline.Token).ConfigureAwait(false);
                jsonBytes = JsonSerializer.SerializeToUtf8Bytes(status);
                if (jsonBytes.Length > Constants.MaxMonitorResponseBytes - Constants.AesGcmMinimumPayloadBytes)
                    throw new InvalidDataException("Monitor status response exceeds the protocol limit.");

                encryptedResponse = CryptoHelper.EncryptAesGcm(jsonBytes);
                await MonitorFrameCodec.WriteAsync(
                    stream,
                    encryptedResponse,
                    Constants.MaxMonitorResponseBytes,
                    deadline.Token,
                    StreamIdleDeadline).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (shutdownToken.IsCancellationRequested) { }
            catch (OperationCanceledException)
            {
                AppLogger.Log($"[MONITOR SERVER] Request deadline reached for {remoteIp}.");
            }
            catch (Exception ex) when (ex is IOException or SocketException or TimeoutException or InvalidDataException or JsonException)
            {
                AppLogger.Log($"[MONITOR SERVER] Request from {remoteIp} failed: {ex.GetType().Name}.");
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[MONITOR SERVER] Unexpected request failure from {remoteIp}", ex);
            }
            finally
            {
                Zero(encryptedRequest);
                Zero(decryptedRequest);
                Zero(jsonBytes);
                Zero(encryptedResponse);
                _concurrencySlot.Release();
            }
        }
    }

    private bool IsRateLimited(string ip)
    {
        long now = Environment.TickCount64;
        if (!_rateLimiter.ContainsKey(ip) && _rateLimiter.Count >= MaxRateLimitEntries)
            PruneRateLimiter(now);
        if (!_rateLimiter.ContainsKey(ip) && _rateLimiter.Count >= MaxRateLimitEntries)
            return true;

        RateLimitEntry entry = _rateLimiter.GetOrAdd(ip, _ => new RateLimitEntry { WindowStart = now });
        lock (entry)
        {
            if (now - entry.WindowStart >= RateLimitWindow.TotalMilliseconds)
            {
                entry.WindowStart = now;
                entry.Count = 0;
            }
            return ++entry.Count > MaxRequestsPerWindow;
        }
    }

    private void PruneRateLimiter(long now)
    {
        foreach (var pair in _rateLimiter)
        {
            if (now - pair.Value.WindowStart >= RateLimitWindow.TotalMilliseconds * 2)
                _rateLimiter.TryRemove(pair.Key, out _);
        }
    }

    private static string GetRemoteIp(TcpClient client)
    {
        try
        {
            return ((IPEndPoint?)client.Client.RemoteEndPoint)?.Address.ToString() ?? "unknown";
        }
        catch
        {
            return "unknown";
        }
    }

    private Task<ServerStatusPayload>? StartStatusWorker(CancellationToken token)
    {
        if (!_statusWorkerSlots.Wait(0))
            return null;

        int workerId = Interlocked.Increment(ref _nextStatusWorkerId);
        Task<ServerStatusPayload> worker = Task.Run(() =>
        {
            try
            {
                return _statusFactory(token);
            }
            finally
            {
                _statusWorkerSlots.Release();
            }
        }, CancellationToken.None);
        _statusWorkerTasks[workerId] = worker;
        _ = worker.ContinueWith(
            (completed, state) =>
            {
                _ = completed.Exception;
                var tuple = ((ConcurrentDictionary<int, Task> Tasks, int Id))state!;
                tuple.Tasks.TryRemove(tuple.Id, out Task? _);
            },
            (_statusWorkerTasks, workerId),
            CancellationToken.None,
            TaskContinuationOptions.ExecuteSynchronously,
            TaskScheduler.Default);
        return worker;
    }

    private static void Zero(byte[]? buffer)
    {
        if (buffer != null)
            CryptographicOperations.ZeroMemory(buffer);
    }

    public void Dispose()
    {
        if (_disposed)
            return;
        Stop();
        _disposed = true;
        GC.SuppressFinalize(this);
    }
}
