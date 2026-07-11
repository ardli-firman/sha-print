using System;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Server;
using ShaPrint.WpfApp.Services;
using Xunit;

namespace ShaPrint.Tests
{
    [Collection("SequentialNetworkTests")]
    public class DiscoveryBypassTests
    {
        private class MockNotificationService : INotificationService
        {
            public int ClientConnectedCount { get; set; }
            public int ClientDisconnectedCount { get; set; }

            public void ShowClientConnected(string clientIp)
            {
                ClientConnectedCount++;
            }

            public void ShowClientDisconnected(string clientIp)
            {
                ClientDisconnectedCount++;
            }

            public void ShowPrintJobCompleted(string documentName, string printerName) { }
            public void ShowPrintJobFailed(string documentName, string printerName, string reason) { }
            public void ShowScanCompleted(string fileName) { }
            public void ShowScanFailed(string errorMessage) { }
            public void ShowPrinterError(string printerName, string errorDescription) { }
            public void ShowSecurityAlert(string message, string detail) { }
            public void ShowToast(string title, string body, ToastAction? action = null) { }
        }

        [Fact]
        public async Task DiscoveryServer_BypassesNotification_ForMonitorRequest()
        {
            // Part 1: Verify Monitor discovery request bypasses notification
            var mockNotification1 = new MockNotificationService();
            var discoveryServer1 = new DiscoveryServer(mockNotification1);

            try
            {
                discoveryServer1.Start();
                await Task.Delay(100);

                using (var udpClient = new UdpClient())
                {
                    byte[] monitorRequestBytes = Encoding.UTF8.GetBytes(Constants.MonitorDiscoveryRequestMessage);
                    await udpClient.SendAsync(monitorRequestBytes, monitorRequestBytes.Length, new IPEndPoint(IPAddress.Loopback, Constants.DiscoveryUdpPort));

                    var receiveTask = udpClient.ReceiveAsync();
                    var completedTask = await Task.WhenAny(receiveTask, Task.Delay(2000));
                    Assert.Equal(receiveTask, completedTask);
                }

                Assert.Equal(0, mockNotification1.ClientConnectedCount);
            }
            finally
            {
                discoveryServer1.Stop();
            }

            // Give a tiny delay for port releasing
            await Task.Delay(100);

            // Part 2: Verify Standard discovery request triggers notification
            var mockNotification2 = new MockNotificationService();
            var discoveryServer2 = new DiscoveryServer(mockNotification2);

            try
            {
                discoveryServer2.Start();
                await Task.Delay(100);

                using (var udpClient = new UdpClient())
                {
                    byte[] standardRequestBytes = Encoding.UTF8.GetBytes(Constants.DiscoveryRequestMessage);
                    await udpClient.SendAsync(standardRequestBytes, standardRequestBytes.Length, new IPEndPoint(IPAddress.Loopback, Constants.DiscoveryUdpPort));

                    var receiveTask = udpClient.ReceiveAsync();
                    var completedTask = await Task.WhenAny(receiveTask, Task.Delay(2000));
                    Assert.Equal(receiveTask, completedTask);
                }

                Assert.Equal(1, mockNotification2.ClientConnectedCount);
            }
            finally
            {
                discoveryServer2.Stop();
            }
        }
        [Fact]
        public async Task DiscoveryClient_UnicastToLocalhost_Succeeds()
        {
            var mockNotification = new MockNotificationService();
            var discoveryServer = new DiscoveryServer(mockNotification);
            
            try
            {
                discoveryServer.Start();
                await Task.Delay(100);
                
                var client = new ShaPrint.Client.DiscoveryClient();
                // Act: explicitly provide loopback IP
                var servers = await client.DiscoverServersAsync("127.0.0.1", timeoutMs: 1000);
                
                // Assert
                Assert.NotEmpty(servers);
            }
            finally
            {
                discoveryServer.Stop();
            }
        }
    }
}
