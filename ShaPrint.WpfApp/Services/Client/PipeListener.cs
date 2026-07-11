using System;

using System.IO;

using System.IO.Pipes;

using System.Net.Sockets;

using System.Threading;

using System.Threading.Tasks;

using System.Printing;

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

        private string _localPrinterName;

        /// <summary>
        /// Fires when SendToServerAsync cannot reach the server. Parameterless by design —
        /// the unreachable IP is captured in the log line for diagnostics. Subscribed by
        /// ServerReachabilityTracker. NOT fired on a successful send.
        /// </summary>
        public event Action? OnServerUnreachable;



        public PipeListener(string pipeName, string serverIp, string targetPrinterName, string localPrinterName)

        {



            PipeName = pipeName;

            _serverIp = serverIp;

            _targetPrinterName = targetPrinterName;

            _localPrinterName = localPrinterName;

        }



        private Task? _listenTask;

        public void Start()

        {

            if (_listenTask != null && !_listenTask.IsCompleted) return;
            _cts = new CancellationTokenSource();
            _listenTask = Task.Run(() => ListenLoopAsync(_cts.Token));

        }



        public void Stop()

        {

            _cts?.Cancel();
            if (_listenTask != null)
            {
                try { _listenTask.Wait(TimeSpan.FromSeconds(2)); }
                catch { }
                _listenTask = null;
            }

        }



        private async Task ListenLoopAsync(CancellationToken token)

        {

            string pipeNameOnly = PipeName.Replace(@"\\.\pipe\", "");

            while (!token.IsCancellationRequested)

            {

                try

                {

                    // Allow SYSTEM and Standard Users to write to this pipe (crucial since we run as Admin!)

                    var pipeSecurity = new System.IO.Pipes.PipeSecurity();

                    pipeSecurity.AddAccessRule(new System.IO.Pipes.PipeAccessRule(

                        new System.Security.Principal.SecurityIdentifier(System.Security.Principal.WellKnownSidType.WorldSid, null),

                        System.IO.Pipes.PipeAccessRights.FullControl,

                        System.Security.AccessControl.AccessControlType.Allow));



                    using var pipeServer = System.IO.Pipes.NamedPipeServerStreamAcl.Create(

                        pipeNameOnly,

                        PipeDirection.In,

                        1,

                        PipeTransmissionMode.Byte,

                        PipeOptions.Asynchronous,

                        0,

                        0,

                        pipeSecurity);



                    using var ctr = token.Register(() => pipeServer.Dispose());
                    try { await pipeServer.WaitForConnectionAsync(token); }
                    catch (ObjectDisposedException) { throw new OperationCanceledException(); }

                    string documentName = GetActiveDocumentName();



                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Caught new print job on pipe: {pipeNameOnly} (Document: {documentName})");

                    using var ms = new MemoryStream();

                    await pipeServer.CopyToAsync(ms, token);

                    byte[] spoolData = ms.ToArray();

                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Read {spoolData.Length} bytes from Windows Spooler.");



                    if (spoolData.Length > 0)

                    {

                        await SendToServerAsync(spoolData, documentName);

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



                private string GetActiveDocumentName()

        {

            try

            {

                using (var printServer = new LocalPrintServer())

                using (var queue = printServer.GetPrintQueue(_localPrinterName))

                {

                    var jobs = queue.GetPrintJobInfoCollection();

                    foreach (var job in jobs)

                    {

                        if (!string.IsNullOrEmpty(job.Name) && !job.IsDeleted && !job.IsCompleted)

                        {

                            return job.Name;

                        }

                    }

                }

            }

            catch (Exception ex)

            {

                ShaPrint.Core.AppLogger.Log($"[CLIENT] Warning: could not query spooler document name: {ex.Message}");

            }

            return "ShaPrint Job - " + DateTime.Now.ToString("yyyyMMdd_HHmmss");

        }



        private async Task SendToServerAsync(byte[] spoolData, string documentName)

        {

            try

            {

                ShaPrint.Core.AppLogger.Log($"[CLIENT] Connecting to Server at {_serverIp}:{Constants.PrintTcpPort}");

                using var tcpClient = new TcpClient();

                await tcpClient.ConnectAsync(_serverIp, Constants.PrintTcpPort);

                ShaPrint.Core.AppLogger.Log($"[CLIENT] Connected. Sending payload");

                using var stream = tcpClient.GetStream();



                var payload = new PrintJobPayload

                {

                    TargetPrinterName = _targetPrinterName,

                    DocumentName = documentName,

                    SpoolData = spoolData

                };



                await PrintJobPayload.WriteAsync(stream, payload);

                ShaPrint.Core.AppLogger.Log($"[CLIENT] Successfully sent {spoolData.Length} bytes to Server for printer: {_targetPrinterName}");

            }

            catch (Exception ex)

            {

                ShaPrint.Core.AppLogger.Error($"[CLIENT] Failed to send print job to server {_serverIp}: " + ex.Message);
                try
                {
                    OnServerUnreachable?.Invoke();
                }
                catch (Exception invokeEx)
                {
                    ShaPrint.Core.AppLogger.Error("[CLIENT] Error invoking OnServerUnreachable: " + invokeEx.Message);
                }

            }

        }

    }

}

