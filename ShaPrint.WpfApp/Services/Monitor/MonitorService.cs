using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Client;
using ShaPrint.WpfApp.ViewModels.Pages;
using CommunityToolkit.Mvvm.Input;

namespace ShaPrint.WpfApp.Services.Monitor
{
    public class MonitorService
    {
        private readonly MonitorViewModel _monitorViewModel;
        private readonly DiscoveryClient _discoveryClient;
        private CancellationTokenSource? _cts;
        private int _isRefreshing = 0;
        private readonly List<Task> _inFlightTasks = new();
        private readonly object _inFlightLock = new();

        public MonitorService(MonitorViewModel monitorViewModel)
        {
            _monitorViewModel = monitorViewModel;
            _monitorViewModel.RefreshCommand = new AsyncRelayCommand(TriggerManualRefreshAsync);
            _discoveryClient = new DiscoveryClient();
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
            // Ensure all discovered servers exist in the ViewModel
            _monitorViewModel.RegisterDiscoveredServers(discoveredServers);

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
            _monitorViewModel.FlagUndiscoveredServers(discoveredServers);
            
            _monitorViewModel.LastRefreshTime = DateTime.UtcNow;
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
                    _monitorViewModel.UpdateServerStatus(payload, ipAddress, isOnline: true);
                }
            }
            catch (OperationCanceledException) when (token.IsCancellationRequested)
            {
                // Graceful cancellation on service stop, ignore
            }
            catch (System.Security.Cryptography.CryptographicException ex)
            {
                AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) auth mismatch: {ex.Message}");
                _monitorViewModel.UpdateServerFailure(hostName, ipAddress, "AuthMismatch");
            }
            catch (Exception ex) when (ex is IOException || ex is SocketException || ex is TimeoutException)
            {
                AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) is unreachable: {ex.Message}");
                _monitorViewModel.UpdateServerFailure(hostName, ipAddress, "Unreachable");
            }
            catch (Exception ex) when (ex is JsonException || ex is InvalidDataException)
            {
                AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) protocol error: {ex.Message}");
                _monitorViewModel.UpdateServerFailure(hostName, ipAddress, "Unreachable");
            }
            catch (Exception ex)
            {
                AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) unexpected error: {ex.Message}");
                _monitorViewModel.UpdateServerFailure(hostName, ipAddress, "Unreachable");
            }
        }
    }
}
