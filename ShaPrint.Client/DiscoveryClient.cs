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
        public async Task<List<DiscoveryResponseMessage>> DiscoverServersAsync(int timeoutMs = 2000)
        {
            var servers = new List<DiscoveryResponseMessage>();
            using var udpClient = new UdpClient();
            udpClient.EnableBroadcast = true;
            
            var endpoint = new IPEndPoint(IPAddress.Broadcast, Constants.DiscoveryUdpPort);
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
