using System;
using System.IO;
using System.IO.Pipes;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Client
{
    public class PipeListener
    {
        public string PipeName { get; private set; }
        private string _serverIp;
        private string _targetPrinterName;
        private CancellationTokenSource? _cts;

        public PipeListener(string pipeName, string serverIp, string targetPrinterName)
        {
            PipeName = pipeName;
            _serverIp = serverIp;
            _targetPrinterName = targetPrinterName;
        }

        public void Start()
        {
            _cts = new CancellationTokenSource();
            Task.Run(() => ListenLoopAsync(_cts.Token));
        }

        public void Stop()
        {
            _cts?.Cancel();
        }

        private async Task ListenLoopAsync(CancellationToken token)
        {
            string pipeNameOnly = PipeName.Replace(@"\\.\pipe\", "");
            while (!token.IsCancellationRequested)
            {
                try
                {
                    using var pipeServer = new NamedPipeServerStream(pipeNameOnly, PipeDirection.In, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
                    await pipeServer.WaitForConnectionAsync(token);

                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Caught new print job on pipe: {pipeNameOnly}");
                    using var ms = new MemoryStream();
                    await pipeServer.CopyToAsync(ms, token);
                    byte[] spoolData = ms.ToArray();
                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Read {spoolData.Length} bytes from Windows Spooler.");

                    if (spoolData.Length > 0)
                    {
                        await SendToServerAsync(spoolData);
                    }
                    
                    pipeServer.Disconnect();
                }
                catch (OperationCanceledException) { break; }
                catch (Exception ex)
                {
                    ShaPrint.Core.AppLogger.Log("[CLIENT] Pipe error: " + ex.Message);
                    await Task.Delay(1000); // Backoff on error
                }
            }
        }

        private async Task SendToServerAsync(byte[] spoolData)
        {
            try
            {
                ShaPrint.Core.AppLogger.Log($"[CLIENT] Connecting to Server at {_serverIp}:{Constants.PrintTcpPort}...");
                using var tcpClient = new TcpClient();
                await tcpClient.ConnectAsync(_serverIp, Constants.PrintTcpPort);
                ShaPrint.Core.AppLogger.Log($"[CLIENT] Connected. Sending payload...");
                using var stream = tcpClient.GetStream();

                var payload = new PrintJobPayload
                {
                    TargetPrinterName = _targetPrinterName,
                    SpoolData = spoolData
                };

                await PrintJobPayload.WriteAsync(stream, payload);
                ShaPrint.Core.AppLogger.Log($"[CLIENT] Successfully sent {spoolData.Length} bytes to Server for printer: {_targetPrinterName}");
            }
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error($"[CLIENT] Failed to send print job to server: " + ex.Message);
            }
        }
    }
}
