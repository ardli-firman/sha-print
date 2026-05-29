using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Sockets;
using System.Net.NetworkInformation;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Client
{
    public class DiscoveryClient
    {
        public async Task<List<DiscoveryResponseMessage>> DiscoverServersAsync(string? targetIp = null, int timeoutMs = 2000)
        {
            var servers = new List<DiscoveryResponseMessage>();
            using var udpClient = new UdpClient();
            udpClient.EnableBroadcast = true;
            
            IPAddress ip = string.IsNullOrWhiteSpace(targetIp) ? IPAddress.Broadcast : IPAddress.Parse(targetIp);
            byte[] requestData = Encoding.UTF8.GetBytes(Constants.DiscoveryRequestMessage);
            
            if (string.IsNullOrWhiteSpace(targetIp))
            {
                foreach (var ni in NetworkInterface.GetAllNetworkInterfaces().Where(n => n.OperationalStatus == OperationalStatus.Up && n.Supports(NetworkInterfaceComponent.IPv4)))
                {
                    foreach (var addr in ni.GetIPProperties().UnicastAddresses.Where(a => a.Address.AddressFamily == AddressFamily.InterNetwork))
                    {
                        var mask = addr.IPv4Mask;
                        var ipBytes = addr.Address.GetAddressBytes();
                        var maskBytes = mask.GetAddressBytes();
                        var broadcastBytes = new byte[4];
                        for (int i = 0; i < 4; i++) broadcastBytes[i] = (byte)(ipBytes[i] | ~maskBytes[i]);

                        var broadcastIp = new IPAddress(broadcastBytes);
                        await udpClient.SendAsync(requestData, requestData.Length, new IPEndPoint(broadcastIp, Constants.DiscoveryUdpPort));
                        
                        uint ipInt = (uint)ipBytes[0] << 24 | (uint)ipBytes[1] << 16 | (uint)ipBytes[2] << 8 | (uint)ipBytes[3];
                        uint maskInt = (uint)maskBytes[0] << 24 | (uint)maskBytes[1] << 16 | (uint)maskBytes[2] << 8 | (uint)maskBytes[3];
                        uint networkInt = ipInt & maskInt;
                        uint broadcastInt = networkInt | ~maskInt;
                        
                        uint hostCount = ~maskInt;
                        if (hostCount > 0 && hostCount <= 1024)
                        {
                            for (uint i = networkInt + 1; i < broadcastInt; i++)
                            {
                                byte[] targetIpBytes = new byte[] { (byte)(i >> 24), (byte)(i >> 16), (byte)(i >> 8), (byte)i };
                                var targetEp = new IPEndPoint(new IPAddress(targetIpBytes), Constants.DiscoveryUdpPort);
                                _ = udpClient.SendAsync(requestData, requestData.Length, targetEp);
                            }
                        }
                    }
                }
            }
            else
            {
                await udpClient.SendAsync(requestData, requestData.Length, new IPEndPoint(ip, Constants.DiscoveryUdpPort));
            }

            var tcs = new TaskCompletionSource<bool>();
            _ = Task.Delay(timeoutMs).ContinueWith(t => tcs.TrySetResult(true));

            _ = Task.Run(async () =>
            {
                try
                {
                    while (!tcs.Task.IsCompleted)
                    {
                        var result = await udpClient.ReceiveAsync();
                        string jsonResponse = Encoding.UTF8.GetString(result.Buffer);

                        // Parse without signature first
                        var response = JsonSerializer.Deserialize<DiscoveryResponseMessage>(jsonResponse);
                        if (response == null)
                            continue;

                        // HMAC verification
                        if (!string.IsNullOrEmpty(response.HmacSignature))
                        {
                            // Reconstruct the JSON that was signed (without HmacSignature)
                            string savedSig = response.HmacSignature;
                            response.HmacSignature = null;
                            string unsignedJson = JsonSerializer.Serialize(response);

                            if (!CryptoHelper.VerifyHmac(Encoding.UTF8.GetBytes(unsignedJson), savedSig))
                            {
                                AppLogger.Log($"[DISCOVERY] HMAC verification failed for response from {result.RemoteEndPoint.Address}. Rejecting.");
                                continue; // Drop unauthenticated response
                            }

                            // Restore signature for completeness
                            response.HmacSignature = savedSig;
                        }
                        else
                        {
                            // Legacy response without HMAC — accept but warn
                            AppLogger.Log($"[DISCOVERY] Warning: received unsigned response from {result.RemoteEndPoint.Address}.");
                        }

                        // Overwrite with the actual reachable IP address from the packet source
                        response.IpAddress = result.RemoteEndPoint.Address.ToString();
                        servers.Add(response);
                    }
                }
                catch (ObjectDisposedException) { /* udpClient closed, normal */ }
                catch (SocketException ex) when (ex.SocketErrorCode == SocketError.OperationAborted || ex.ErrorCode == 995) { /* udpClient closed, normal */ }
                catch (Exception ex) { AppLogger.Error("Discovery receive error", ex); }
            });

            await tcs.Task;
            udpClient.Close();
            return servers;
        }
    }
}
