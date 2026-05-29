using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Sockets;
using System.Net.NetworkInformation;
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
            
            if (ip.Equals(IPAddress.Broadcast))
            {
                // Send standard 255.255.255.255 broadcast
                await udpClient.SendAsync(requestData, requestData.Length, new IPEndPoint(IPAddress.Broadcast, Constants.DiscoveryUdpPort));
                
                // Send to all specific subnet broadcast addresses to ensure it goes out on the physical LAN, 
                // bypassing Windows routing issues with multiple adapters (like WSL, VirtualBox, VPNs)
                try
                {
                    foreach (var ni in NetworkInterface.GetAllNetworkInterfaces())
                    {
                        if (ni.OperationalStatus == OperationalStatus.Up && 
                            ni.NetworkInterfaceType != NetworkInterfaceType.Loopback)
                        {
                            foreach (var uipi in ni.GetIPProperties().UnicastAddresses)
                            {
                                if (uipi.Address.AddressFamily == AddressFamily.InterNetwork)
                                {
                                    var mask = uipi.IPv4Mask;
                                    if (mask != null && !mask.Equals(IPAddress.Any))
                                    {
                                        byte[] ipBytes = uipi.Address.GetAddressBytes();
                                        byte[] maskBytes = mask.GetAddressBytes();
                                        byte[] broadcastBytes = new byte[ipBytes.Length];
                                        for (int i = 0; i < broadcastBytes.Length; i++)
                                        {
                                            broadcastBytes[i] = (byte)(ipBytes[i] | (maskBytes[i] ^ 255));
                                        }
                                        var broadcastIp = new IPAddress(broadcastBytes);
                                        await udpClient.SendAsync(requestData, requestData.Length, new IPEndPoint(broadcastIp, Constants.DiscoveryUdpPort));
                                    }
                                }
                            }
                        }
                    }
                }
                catch (Exception ex)
                {
                    AppLogger.Log($"[DISCOVERY] Error enumerating network interfaces: {ex.Message}");
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
