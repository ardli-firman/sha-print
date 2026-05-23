using System;
using System.Net;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Server
{
    public class PrintReceiver
    {
        private TcpListener? _listener;
        private CancellationTokenSource? _cts;

        public void Start()
        {
            _cts = new CancellationTokenSource();
            _listener = new TcpListener(IPAddress.Any, Constants.PrintTcpPort);
            _listener.Start();
            Task.Run(() => AcceptLoopAsync(_cts.Token));
        }

        public void Stop()
        {
            _cts?.Cancel();
            _listener?.Stop();
        }

        private async Task AcceptLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    var client = await _listener.AcceptTcpClientAsync();
                    _ = Task.Run(() => HandleClientAsync(client, token));
                }
                catch (ObjectDisposedException) { break; }
                catch (Exception) { }
            }
        }

        private async Task HandleClientAsync(TcpClient client, CancellationToken token)
        {
            using (client)
            using (var stream = client.GetStream())
            {
                try
                {
                    var payload = await PrintJobPayload.ReadAsync(stream);
                    if (!string.IsNullOrEmpty(payload.TargetPrinterName) && payload.SpoolData != null && payload.SpoolData.Length > 0)
                    {
                        string docName = "ShaPrint Job - " + DateTime.Now.ToString("yyyyMMdd_HHmmss");
                        bool printed = SpoolerApi.PrintRawData(payload.TargetPrinterName, payload.SpoolData, docName);
                        Console.WriteLine($"Print job for {payload.TargetPrinterName} received. Success: {printed}");
                    }
                }
                catch (Exception ex)
                {
                    Console.WriteLine("Error handling print job: " + ex.Message);
                }
            }
        }
    }
}
