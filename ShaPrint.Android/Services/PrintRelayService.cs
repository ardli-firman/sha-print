using System.Net;
using System.Net.Sockets;
using Android.Content;
using Android.Provider;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;
// Namespace ShaPrint.Android shadows the global `Android` namespace for inline qualified
// names (CS0234 otherwise), so Android.Net.Uri is aliased via global::.
using AndroidUri = global::Android.Net.Uri;

namespace ShaPrint.Android.Services;

/// <summary>Result of the SAF file picker: raw bytes + best-effort display name.</summary>
public sealed record PickedDocument(byte[] Data, string DocumentName);

/// <summary>
/// <see cref="IPrintRelayClient"/> for Android plus the Storage Access Framework
/// (ACTION_OPEN_DOCUMENT) file picker.
///
/// The relay wire logic mirrors <c>ShaPrint.Platform.Unix.UnixPrintRelayClient</c> (Task 7),
/// which is pure .NET (sockets over <c>ShaPrint.Core</c>); the file picker is Android-only and
/// delegated to <see cref="MainActivity"/> (an Activity is required for the picker intent).
///
/// Flow (Task 9 step 5):
/// <code>
/// PickDocumentAsync() → SAF picker → bytes + name
///   → SendAsync(printer, bytes, name, hostOverride?)
///   → host = hostOverride ?? DiscoveryService (first server exposing targetPrinter)
///   → PrintJobPayload.WriteAsync() — AES-256-GCM (ShaPrint.Core) over TCP 9877
/// </code>
/// </summary>
public sealed class PrintRelayService : IPrintRelayClient
{
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(10);

    private readonly DiscoveryService _discovery;
    private readonly Context _appContext;

    public PrintRelayService(DiscoveryService discovery, Context appContext)
    {
        _discovery = discovery;
        _appContext = appContext;
    }

    /// <summary>
    /// Opens the system document picker and reads the picked document fully into memory.
    /// Returns null when the user cancels or the document cannot be read; enforces the
    /// <see cref="Constants.MaxPrintJobBytes"/> cap so a huge pick cannot OOM the relay.
    /// </summary>
    public async Task<PickedDocument?> PickDocumentAsync()
    {
        var activity = MainActivity.Current
            ?? throw new InvalidOperationException(
                "MainActivity is not available yet — file picking requires an Activity.");

        AndroidUri? uri = await activity.PickDocumentAsync();
        if (uri == null)
        {
            AppLogger.Log("[RELAY] File picker cancelled.");
            return null;
        }

        byte[] data;
        try
        {
            using var stream = _appContext.ContentResolver!.OpenInputStream(uri);
            if (stream == null)
            {
                AppLogger.Error("[RELAY] Could not open picked file (null stream).");
                return null;
            }

            using var ms = new MemoryStream();
            await stream.CopyToAsync(ms);
            data = ms.ToArray();
        }
        catch (Exception ex)
        {
            AppLogger.Error("[RELAY] Failed to read picked file: " + ex.Message, ex);
            return null;
        }

        if (data.Length > Constants.MaxPrintJobBytes)
        {
            AppLogger.Error($"[RELAY] Picked file is {data.Length:N0} bytes — over the " +
                            $"{Constants.MaxPrintJobBytes:N0} byte limit. Rejecting.");
            return null;
        }

        return new PickedDocument(data, QueryDisplayName(uri) ?? "android-document");
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
        }
        if (string.IsNullOrEmpty(host))
        {
            AppLogger.Error($"[RELAY] No ShaPrint server advertising printer '{targetPrinter}' found via UDP discovery. " +
                            "Make sure a ShaPrint server exposing that printer is running (or pass an explicit host).");
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
    /// Resolves the target server host via <see cref="DiscoveryService"/>. When several
    /// servers advertise the printer, the first discovered match wins (an explicit host
    /// override disambiguates) — same semantics as the shared <c>PrintRelayClientService</c>.
    /// </summary>
    private async Task<string?> ResolveServerHostAsync(string targetPrinter, CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();

        var servers = await _discovery.DiscoverServersAsync();

        var match = servers.FirstOrDefault(s =>
            s.ExposedPrinters != null &&
            s.ExposedPrinters.Any(p => string.Equals(p.Name, targetPrinter, StringComparison.OrdinalIgnoreCase)));

        return match?.IpAddress;
    }

    /// <summary>
    /// Parses <paramref name="host"/> as an IP address, falling back to DNS resolution so a
    /// hostname (e.g. <c>my-server.local</c>) works as well. Returns null when neither works.
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
    /// Best-effort display name from the SAF picker (<see cref="IOpenableColumns.DisplayName"/>).
    /// Falls back to the caller's default when the provider does not expose it.
    /// </summary>
    private string? QueryDisplayName(AndroidUri uri)
    {
        try
        {
            var projection = new[] { IOpenableColumns.DisplayName };
            using var cursor = _appContext.ContentResolver?.Query(uri, projection, null, null, null);
            if (cursor != null && cursor.MoveToFirst())
            {
                int idx = cursor.GetColumnIndex(IOpenableColumns.DisplayName);
                if (idx >= 0)
                {
                    return cursor.GetString(idx);
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Log($"[RELAY] Could not query display name: {ex.Message}");
        }

        return null;
    }
}
