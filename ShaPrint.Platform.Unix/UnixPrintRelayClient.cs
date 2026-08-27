using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Unix;

/// <summary>
/// <see cref="IPrintRelayClient"/> for macOS/Linux: relays an encrypted
/// <see cref="PrintJobPayload"/> (AES-256-GCM) to a ShaPrint server over TCP 9877.
///
/// CIRCULAR-REFERENCE RESOLUTION — option (a), see task report: <c>ShaPrint.UI</c>
/// references this project (for <c>AddPlatformUnix</c>), so this project must NOT reference
/// <c>ShaPrint.UI</c> (whose richer <c>DiscoveryClientService</c> lives there). Instead this
/// class performs a compact inline UDP discovery (broadcast to 9876 + HMAC-SHA256
/// verification of <see cref="DiscoveryResponseMessage"/>, using only <c>ShaPrint.Core</c>
/// types — the logic mirrors <c>DiscoveryClientService</c>). In the normal app flow the
/// shared <c>PrintRelayClientService</c> (ShaPrint.UI) already resolves the host and passes
/// it via <paramref name="hostOverride"/>, so this fallback only runs for direct use.
///
/// Pure .NET sockets — no OS-specific API, so no Unix runtime guard is required here.
/// </summary>
public sealed class UnixPrintRelayClient : IPrintRelayClient
{
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan DiscoveryTimeout = TimeSpan.FromSeconds(2.5);

    public async Task<bool> SendAsync(
        string targetPrinter,
        byte[] data,
        string documentName,
        string? hostOverride = null,
        CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(targetPrinter))
        {
            throw new ArgumentException("Target printer name is required.", nameof(targetPrinter));
        }

        if (data == null || data.Length == 0)
        {
            return false;
        }

        ct.ThrowIfCancellationRequested();

        string? host = hostOverride?.Trim();
        if (string.IsNullOrEmpty(host))
        {
            host = await DiscoverServerHostAsync(targetPrinter, ct);
        }
        if (string.IsNullOrEmpty(host))
        {
            AppLogger.Error($"[RELAY] No ShaPrint server advertising printer '{targetPrinter}' found via UDP discovery. " +
                            "Pass an explicit host, or make sure a ShaPrint server exposing that printer is running.");
            return false;
        }

        try
        {
            IPAddress? address = ResolveHost(host);
            if (address == null)
            {
                return false; // already logged by ResolveHost
            }

            using var client = new TcpClient();
            using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
            timeoutCts.CancelAfter(ConnectTimeout);
            await client.ConnectAsync(address, Constants.PrintTcpPort, timeoutCts.Token);

            await using var stream = client.GetStream();
            var payload = new PrintJobPayload
            {
                TargetPrinterName = targetPrinter,
                DocumentName = documentName,
                SpoolData = data,
            };
            await PrintJobPayload.WriteAsync(stream, payload);

            AppLogger.Log($"[RELAY] Sent {data.Length} bytes to {host}:{Constants.PrintTcpPort} for printer '{targetPrinter}'.");
            return true;
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[RELAY] Send to {host}:{Constants.PrintTcpPort} failed: {ex.Message}");
            return false;
        }
    }

    /// <summary>
    /// Parses <paramref name="host"/> as an IP address, falling back to DNS resolution so a
    /// hostname (e.g. <c>my-server.local</c>) passed via <paramref name="hostOverride"/>
    /// works as well. Returns null when neither parse nor DNS succeeds.
    /// </summary>
    private static IPAddress? ResolveHost(string host)
    {
        if (IPAddress.TryParse(host, out var parsed))
        {
            return parsed;
        }

        try
        {
            return Dns.GetHostAddresses(host).FirstOrDefault(a => a.AddressFamily == AddressFamily.InterNetwork);
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[RELAY] Could not resolve host '{host}': {ex.Message}");
            return null;
        }
    }

    /// <summary>
    /// Compact UDP discovery: broadcasts <see cref="Constants.DiscoveryRequestMessage"/> on
    /// port 9876, verifies each <see cref="DiscoveryResponseMessage"/> HMAC-SHA256 signature
    /// (exactly like <c>ShaPrint.UI.Services.DiscoveryClientService</c>), and returns the
    /// first server exposing <paramref name="targetPrinter"/>. No network-interface sweep in
    /// this fallback — the UI orchestration path performs the richer sweep.
    /// </summary>
    private async Task<string?> DiscoverServerHostAsync(string targetPrinter, CancellationToken ct)
    {
        try
        {
            using var udp = new UdpClient();
            udp.EnableBroadcast = true;

            byte[] request = Encoding.UTF8.GetBytes(Constants.DiscoveryRequestMessage);
            await udp.SendAsync(request, request.Length,
                new IPEndPoint(IPAddress.Broadcast, Constants.DiscoveryUdpPort));

            var deadline = DateTime.UtcNow + DiscoveryTimeout;
            while (DateTime.UtcNow < deadline)
            {
                ct.ThrowIfCancellationRequested();

                try
                {
                    var recvTask = udp.ReceiveAsync();
                    var recv = await recvTask.WaitAsync(TimeSpan.FromMilliseconds(500), ct);

                    if (!TryVerifyResponse(recv.Buffer, out var response))
                    {
                        continue;
                    }

                    // Trust the packet source over the JSON field (same as DiscoveryClientService).
                    response.IpAddress = recv.RemoteEndPoint.Address.ToString();

                    if (response.ExposedPrinters != null &&
                        response.ExposedPrinters.Any(p => string.Equals(p.Name, targetPrinter, StringComparison.OrdinalIgnoreCase)))
                    {
                        return response.IpAddress;
                    }
                }
                catch (OperationCanceledException)
                {
                    throw; // caller cancellation
                }
                catch (TimeoutException)
                {
                    // per-receive timeout — keep listening until the deadline
                }
                catch (SocketException se) when (se.SocketErrorCode is SocketError.ConnectionReset or SocketError.TimedOut)
                {
                    // ICMP port unreachable / socket timeout
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[RELAY] UDP discovery failed: " + ex.Message);
        }

        return null;
    }

    /// <summary>
    /// Parses and HMAC-verifies a discovery response. Mirrors
    /// <c>ShaPrint.UI.Services.DiscoveryClientService</c>: the signature covers the JSON with
    /// the <c>HmacSignature</c> field nulled.
    /// </summary>
    private static bool TryVerifyResponse(byte[] buffer, out DiscoveryResponseMessage response)
    {
        response = null!;
        string json = Encoding.UTF8.GetString(buffer);

        var parsed = JsonSerializer.Deserialize<DiscoveryResponseMessage>(json);
        if (parsed == null)
        {
            return false;
        }

        if (!string.IsNullOrEmpty(parsed.HmacSignature))
        {
            string savedSignature = parsed.HmacSignature;
            parsed.HmacSignature = null;
            string unsignedJson = JsonSerializer.Serialize(parsed);

            if (!CryptoHelper.VerifyHmac(Encoding.UTF8.GetBytes(unsignedJson), savedSignature))
            {
                AppLogger.Log("[RELAY] Discovery response HMAC verification failed — rejecting.");
                return false;
            }

            parsed.HmacSignature = savedSignature;
        }

        response = parsed;
        return true;
    }
}
