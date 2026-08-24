using System;

using System.Buffers;
using System.IO;

using System.IO.Pipes;

using System.Net.Sockets;

using System.Threading;

using System.Threading.Tasks;
using System.Security.Cryptography;

using System.Printing;

using ShaPrint.Core;

using ShaPrint.Core.Network;



namespace ShaPrint.Client

{

    [Obsolete("Deprecated: IPP server handles all printing. Named pipe interception no longer needed.")]

    public class PipeListener

    {
        private static readonly TimeSpan TransportTimeout = TimeSpan.FromSeconds(30);

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
        public bool IsListening { get; private set; }
        public Exception? LastError { get; private set; }



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
            _cts?.Dispose();
            _cts = new CancellationTokenSource();
            _listenTask = Task.Run(() => ListenLoopAsync(_cts.Token));
        }



        public async Task StopAsync()
        {
            _cts?.Cancel();
            if (_listenTask != null)
            {
                try { await Task.WhenAny(_listenTask, Task.Delay(2000)); }
                catch { }
                _listenTask = null;
            }
            _cts?.Dispose();
            _cts = null;
        }

        public void Stop() => _ = StopAsync();



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
                    try 
                    { 
                        IsListening = true;
                        await pipeServer.WaitForConnectionAsync(token); 
                        IsListening = false;
                    }
                    catch (ObjectDisposedException) { throw new OperationCanceledException(); }

                    string documentName = GetActiveDocumentName();



                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Caught new print job on pipe: {pipeNameOnly} (Document: {documentName})");

                    using var transferCts = CancellationTokenSource.CreateLinkedTokenSource(token);
                    transferCts.CancelAfter(TransportTimeout);
                    byte[] spoolData = await ReadBoundedPipePayloadAsync(pipeServer, transferCts.Token);
                    try
                    {
                        ShaPrint.Core.AppLogger.Log($"[CLIENT] Read {spoolData.Length} bytes from Windows Spooler.");
                        if (spoolData.Length > 0)
                            await SendToServerAsync(spoolData, documentName, transferCts.Token);
                    }
                    finally
                    {
                        CryptographicOperations.ZeroMemory(spoolData);
                    }

                    

                    pipeServer.Disconnect();

                }

                catch (OperationCanceledException) { IsListening = false; break; }

                catch (Exception ex)
                {
                    LastError = ex;
                    IsListening = false;
                    AppLogger.Error("Pipe listener error", ex);
                    // Add a tiny delay on error to prevent CPU spin if it repeatedly fails
                    await Task.Delay(100, token);
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



        private async Task SendToServerAsync(byte[] spoolData, string documentName, CancellationToken token)

        {

            try

            {

                ShaPrint.Core.AppLogger.Log($"[CLIENT] Connecting to Server at {_serverIp}:{Constants.PrintTcpPort}");

                using var connectCts = CancellationTokenSource.CreateLinkedTokenSource(token);
                connectCts.CancelAfter(TransportTimeout);
                using var tcpClient = new TcpClient();

                await tcpClient.ConnectAsync(_serverIp, Constants.PrintTcpPort, connectCts.Token);

                ShaPrint.Core.AppLogger.Log($"[CLIENT] Connected. Sending payload");

                using var stream = tcpClient.GetStream();



                var payload = new PrintJobPayload

                {

                    TargetPrinterName = _targetPrinterName,

                    DocumentName = documentName,

                    SpoolData = spoolData

                };



                byte[] payloadWire = PrintJobPayload.Serialize(payload);
                try
                {
                    long correlationId = CreateCorrelationId();
                    var envelope = new LegacyEnvelope(LegacyProtocolVersion.Current, LegacyMessageType.PrintJob, correlationId, payloadWire);
                    await LegacyEnvelopeCodec.WriteAsync(stream, envelope, connectCts.Token);

                    LegacyAcknowledgement acknowledgement = await LegacyAcknowledgementCodec.ReadFramedAsync(stream, connectCts.Token);
                    if (acknowledgement.CorrelationId != correlationId)
                        throw new InvalidDataException("Server acknowledgement correlation ID does not match the submitted print job.");

                    if (acknowledgement.Status != LegacyAcknowledgementStatus.Accepted)
                    {
                        AppLogger.Error($"[CLIENT] Server rejected print job ({acknowledgement.Status}): {acknowledgement.Message}");
                        return;
                    }

                    ShaPrint.Core.AppLogger.Log($"[CLIENT] Server accepted {spoolData.Length} bytes for printer: {_targetPrinterName}");
                }
                finally
                {
                    CryptographicOperations.ZeroMemory(payloadWire);
                }

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

        private static long CreateCorrelationId()
        {
            Span<byte> buffer = stackalloc byte[sizeof(long)];
            long value;
            do
            {
                RandomNumberGenerator.Fill(buffer);
                value = BitConverter.ToInt64(buffer);
            }
            while (value == 0);

            return value;
        }

        internal static async Task<byte[]> ReadBoundedPipePayloadAsync(Stream pipe, CancellationToken token)
        {
            const int bufferSize = 64 * 1024;
            byte[] buffer = ArrayPool<byte>.Shared.Rent(bufferSize);
            using var memory = new MemoryStream();
            try
            {
                int total = 0;
                while (true)
                {
                    int read = await pipe.ReadAsync(buffer.AsMemory(0, bufferSize), token);
                    if (read == 0)
                        break;
                    if (read > Constants.MaxPrintJobBytes - total)
                        throw new InvalidDataException($"Named-pipe print payload exceeds {Constants.MaxPrintJobBytes} bytes.");

                    await memory.WriteAsync(buffer.AsMemory(0, read), token);
                    total += read;
                }

                return memory.ToArray();
            }
            finally
            {
                CryptographicOperations.ZeroMemory(buffer);
                ArrayPool<byte>.Shared.Return(buffer);
                if (memory.TryGetBuffer(out ArraySegment<byte> owned) && owned.Array is not null)
                    CryptographicOperations.ZeroMemory(owned.Array.AsSpan(owned.Offset, (int)memory.Length));
            }
        }

    }

}

