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
            {
                var remoteIp = ((IPEndPoint)client.Client.RemoteEndPoint!).Address.ToString();
                Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] [SERVER] Incoming TCP connection from {remoteIp}");

                using (var stream = client.GetStream())
                {
                    try
                    {
                        var payload = await PrintJobPayload.ReadAsync(stream);
                        Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] [SERVER] Received payload. Printer: '{payload.TargetPrinterName}', Data size: {payload.SpoolData?.Length ?? 0} bytes.");

                        if (!string.IsNullOrEmpty(payload.TargetPrinterName) && payload.SpoolData != null && payload.SpoolData.Length > 0)
                        {
                            string docName = "ShaPrint Job - " + DateTime.Now.ToString("yyyyMMdd_HHmmss");
                            Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] [SERVER] Injecting {payload.SpoolData.Length} bytes into Windows Spooler for '{payload.TargetPrinterName}'...");
                            bool printed = SpoolerApi.PrintRawData(payload.TargetPrinterName, payload.SpoolData, docName);
                            
                            if (printed)
                                Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] [SERVER] SUCCESS: Print job accepted by Windows Spooler.");
                            else
                                Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] [SERVER] FAILED: Windows Spooler rejected the job. Check SpoolerApi logs.");
                        }
                        else
                        {
                            Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] [SERVER] ERROR: Empty payload or missing printer name.");
                        }
                    }
                    catch (Exception ex)
                    {
                        Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] [SERVER] ERROR handling print job: " + ex.Message);
                    }
                }
            }
        }
    }
}
