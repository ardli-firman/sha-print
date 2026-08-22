using System.Net;
using System.Text;

namespace ShaPrint.Core.Ipp;

/// <summary>
/// HTTP server that exposes IIppServer via HTTP POST on port 631.
/// Handles IPP over HTTP protocol as required by Windows IPP client.
/// </summary>
public class IppHttpServer : IDisposable
{
    private readonly IIppServer _ippServer;
    private readonly ISpoolerAdapter _spooler;
    private HttpListener? _listener;
    private CancellationTokenSource? _cts;
    private Task? _listenTask;

    public int Port { get; }
    public bool IsListening => _listener?.IsListening ?? false;

    public IppHttpServer(ISpoolerAdapter spooler, int port = 631)
    {
        _spooler = spooler;
        _ippServer = new IppServer(spooler);
        Port = port;
    }

    /// <summary>
    /// Start the IPP HTTP server.
    /// </summary>
    public void Start()
    {
        if (_listener != null) return;

        _cts = new CancellationTokenSource();
        _listener = new HttpListener();
        _listener.Prefixes.Add($"http://+:{Port}/");
        _listener.Prefixes.Add($"http://+:{Port}/ipp/");
        _listener.Prefixes.Add($"http://+:{Port}/ipp/print/");
        _listener.Prefixes.Add($"http://+:{Port}/printers/");

        try
        {
            _listener.Start();
            _listenTask = Task.Run(() => ListenLoopAsync(_cts.Token));
            AppLogger.Log($"[IPP] IPP HTTP server started on port {Port}");
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[IPP] Failed to start IPP HTTP server: {ex.Message}");
            _listener = null;
            throw;
        }
    }

    /// <summary>
    /// Stop the IPP HTTP server.
    /// </summary>
    public async Task StopAsync()
    {
        _cts?.Cancel();
        _listener?.Stop();

        if (_listenTask != null)
        {
            try
            {
                await Task.WhenAny(_listenTask, Task.Delay(5000));
            }
            catch { }
            _listenTask = null;
        }

        _listener?.Close();
        _listener = null;
        _cts?.Dispose();
        _cts = null;

        AppLogger.Log("[IPP] IPP HTTP server stopped");
    }

    public void Dispose()
    {
        _ = StopAsync();
    }

    private async Task ListenLoopAsync(CancellationToken token)
    {
        while (!token.IsCancellationRequested && _listener?.IsListening == true)
        {
            try
            {
                var context = await _listener.GetContextAsync();
                _ = Task.Run(() => HandleRequestAsync(context, token), token);
            }
            catch (HttpListenerException) when (token.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex) when (!token.IsCancellationRequested)
            {
                AppLogger.Error($"[IPP] Error accepting request: {ex.Message}");
            }
        }
    }

    private async Task HandleRequestAsync(HttpListenerContext context, CancellationToken token)
    {
        var request = context.Request;
        var response = context.Response;

        try
        {
            // Only accept POST requests (IPP uses POST)
            if (request.HttpMethod != "POST")
            {
                response.StatusCode = 405;
                response.StatusDescription = "Method Not Allowed";
                response.Close();
                return;
            }

            // Set response content type to IPP
            response.ContentType = "application/ipp";
            response.Headers.Add("Server", "ShaPrint IPP Server/1.0");

            // Process IPP request
            using var inputStream = request.InputStream;
            using var outputStream = new MemoryStream();

            await _ippServer.ProcessRequestAsync(inputStream, outputStream, token);

            // Write response
            outputStream.Position = 0;
            response.ContentLength64 = outputStream.Length;
            await outputStream.CopyToAsync(response.OutputStream, token);

            AppLogger.Log($"[IPP] Handled {request.Url?.AbsolutePath} from {request.RemoteEndPoint}");
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[IPP] Error handling request: {ex.Message}");
            response.StatusCode = 500;
        }
        finally
        {
            response.Close();
        }
    }
}
