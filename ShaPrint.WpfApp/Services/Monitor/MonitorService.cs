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
        private bool _isRefreshing = false;

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
            AppLogger.Log("[MONITOR SERVICE] Service stopped.");
        }

        public async Task TriggerManualRefreshAsync()
        {
            if (_isRefreshing) return;
            _isRefreshing = true;
            AppLogger.Log("[MONITOR SERVICE] Triggering manual full refresh (with unicast sweep)...");
            try
            {
                // Unicast sweep allowed on manual refresh to find AP isolated servers
                var discovered = await _discoveryClient.DiscoverServersAsync(
                    skipUnicastSweep: false, 
                    requestMessage: Constants.MonitorDiscoveryRequestMessage);
                
                await QueryAllServersStaggeredAsync(discovered, _cts?.Token ?? CancellationToken.None);
            }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR SERVICE] Manual refresh failed", ex);
            }
            finally
            {
                _isRefreshing = false;
            }
        }

        private async Task PollLoopAsync(CancellationToken token)
        {
            // Initial poll at startup
            try
            {
                var initialDiscovered = await _discoveryClient.DiscoverServersAsync(
                    skipUnicastSweep: false, 
                    requestMessage: Constants.MonitorDiscoveryRequestMessage);
                await QueryAllServersStaggeredAsync(initialDiscovered, token);
            }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR SERVICE] Initial discovery failed", ex);
            }

            while (!token.IsCancellationRequested)
            {
                try
                {
                    await Task.Delay(TimeSpan.FromSeconds(15), token);

                    if (_isRefreshing) continue;

                    var discovered = await _discoveryClient.DiscoverServersAsync(
                        skipUnicastSweep: true, 
                        requestMessage: Constants.MonitorDiscoveryRequestMessage);

                    await QueryAllServersStaggeredAsync(discovered, token);
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
                if (token.IsCancellationRequested) break;

                // Fire and forget status check for each server
                _ = QueryServerStatusAsync(server.ServerName, server.IpAddress, token);

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
            catch (Exception ex)
            {
                AppLogger.Log($"[MONITOR SERVICE] Server '{hostName}' ({ipAddress}) is unreachable: {ex.Message}");
                _monitorViewModel.UpdateServerOffline(hostName, ipAddress);
            }
        }
    }
}
