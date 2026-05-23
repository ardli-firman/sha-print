using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Sockets;
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
            var endpoint = new IPEndPoint(ip, Constants.DiscoveryUdpPort);
            byte[] requestData = Encoding.UTF8.GetBytes(Constants.DiscoveryRequestMessage);
            
            await udpClient.SendAsync(requestData, requestData.Length, endpoint);

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
                        var response = JsonSerializer.Deserialize<DiscoveryResponseMessage>(jsonResponse);
                        if (response != null)
                        {
                            // Overwrite with the actual reachable IP address from the packet source
                            response.IpAddress = result.RemoteEndPoint.Address.ToString();
                            servers.Add(response);
                        }
                    }
                }
                catch (ObjectDisposedException) { }
                catch (Exception) { }
            });

            await tcs.Task;
            udpClient.Close();
            return servers;
        }
    }
}
