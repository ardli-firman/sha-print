using System;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Server
{
    public class DiscoveryServer
    {
        private UdpClient? _udpClient;
        private CancellationTokenSource? _cts;
        private List<string> _exposedPrinters = new List<string>();

        public void SetExposedPrinters(List<string> printers)
        {
            _exposedPrinters = printers;
        }

        public void Start()
        {
            _cts = new CancellationTokenSource();
            _udpClient = new UdpClient(Constants.DiscoveryUdpPort);
            Task.Run(() => ListenLoopAsync(_cts.Token));
        }

        public void Stop()
        {
            _cts?.Cancel();
            _udpClient?.Close();
        }

        private async Task ListenLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    var result = await _udpClient.ReceiveAsync();
                    string request = Encoding.UTF8.GetString(result.Buffer);
                    
                    if (request == Constants.DiscoveryRequestMessage)
                    {
                        var response = new DiscoveryResponseMessage
                        {
                            ServerName = Environment.MachineName,
                            IpAddress = GetLocalIPAddress(),
                            ExposedPrinters = _exposedPrinters.Select(p => new PrinterInfo { Name = p, Description = "Shared via ShaPrint" }).ToList()
                        };

                        string jsonResponse = JsonSerializer.Serialize(response);
                        byte[] responseData = Encoding.UTF8.GetBytes(jsonResponse);

                        await _udpClient.SendAsync(responseData, responseData.Length, result.RemoteEndPoint);
                    }
                }
                catch (ObjectDisposedException) { break; }
                catch (Exception) { }
            }
        }

        private string GetLocalIPAddress()
        {
            var host = Dns.GetHostEntry(Dns.GetHostName());
            foreach (var ip in host.AddressList)
            {
                if (ip.AddressFamily == AddressFamily.InterNetwork)
                {
                    return ip.ToString();
                }
            }
            return "127.0.0.1";
        }
    }
}
