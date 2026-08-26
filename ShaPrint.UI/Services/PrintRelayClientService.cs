using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.UI.Services;

/// <summary>
/// Shared print-relay orchestration (used by CLI <c>send</c> in Task 8 and the CUPS backend
/// path). Resolves the target server address and delegates the actual wire transfer to
/// <see cref="IPrintRelayClient"/>:
///
/// <code>
/// SendAsync(targetPrinter, data, documentName, hostOverride)
///   → host = hostOverride ?? discovery (first server exposing targetPrinter)
///   → IPrintRelayClient.SendAsync(..., host)
///   → PrintJobPayload.WriteAsync() — AES-256-GCM (ShaPrint.Core) over TCP 9877
/// </code>
///
/// The concrete <see cref="IPrintRelayClient"/> is resolved via DI (Windows adapter wraps the
/// existing PipeListener send path; Unix adapter added in Task 7). This class itself performs
/// no OS-specific I/O.
/// </summary>
public class PrintRelayClientService
{
    private readonly IPrintRelayClient _relayClient;
    private readonly DiscoveryClientService _discoveryClient;

    public PrintRelayClientService(IPrintRelayClient relayClient, DiscoveryClientService discoveryClient)
    {
        _relayClient = relayClient;
        _discoveryClient = discoveryClient;
    }

    public async Task<bool> SendAsync(
        string targetPrinter,
        byte[] data,
        string documentName,
        string? hostOverride = null,
        CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(targetPrinter))
            throw new ArgumentException("Target printer name is required.", nameof(targetPrinter));

        if (data == null || data.Length == 0)
            return false;

        ct.ThrowIfCancellationRequested();

        string? host = hostOverride?.Trim();
        if (string.IsNullOrEmpty(host))
        {
            host = await ResolveServerHostAsync(targetPrinter, ct);
            if (host == null)
            {
                AppLogger.Error($"[RELAY] No server advertising printer '{targetPrinter}' found on the LAN. " +
                                "Make sure a ShaPrint server exposing that printer is running, or pass an explicit host.");
                return false;
            }
        }

        return await _relayClient.SendAsync(targetPrinter, data, documentName, host, ct);
    }

    /// <summary>
    /// Discovers ShaPrint servers and resolves the host that exposes <paramref name="targetPrinter"/>.
    /// Returns null when no server advertises the printer. When several servers match, the first
    /// discovered server wins and a warning is logged (an explicit host disambiguates).
    /// </summary>
    private async Task<string?> ResolveServerHostAsync(string targetPrinter, CancellationToken ct)
    {
        var servers = await _discoveryClient.DiscoverServersAsync();

        var matches = servers
            .Where(s => s.ExposedPrinters != null &&
                        s.ExposedPrinters.Any(p => string.Equals(p.Name, targetPrinter, StringComparison.OrdinalIgnoreCase)))
            .ToList();

        if (matches.Count == 0)
            return null;

        if (matches.Count > 1)
        {
            AppLogger.Log($"[RELAY] Multiple servers advertise printer '{targetPrinter}' — using first match ({matches[0].IpAddress}). " +
                          "Pass an explicit host to pin the target server.");
        }

        return matches[0].IpAddress;
    }
}