using ShaPrint.Core;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Windows.Adapters;

/// <summary>
/// Adapter implementing <see cref="IPrintRelayClient"/> on Windows by reusing the
/// existing <see cref="PipeListener"/> send path (TCP 9877 + encrypted
/// <see cref="ShaPrint.Core.Network.PrintJobPayload"/>).
///
/// NOTE ON HOST RESOLUTION: server discovery is orchestrated by the caller
/// (ShaPrint.UI's shared <c>PrintRelayClientService</c>, Task 4) and passed via
/// <paramref name="hostOverride"/>. When no host is supplied, this adapter fails
/// explicitly instead of guessing (a localhost default would silently mis-target a
/// remote print job).
/// </summary>
public sealed class WindowsPrintRelayClient : IPrintRelayClient
{
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

        string serverIp = hostOverride?.Trim() ?? string.Empty;
        if (string.IsNullOrEmpty(serverIp))
        {
            AppLogger.Error("[RELAY] SendAsync requires a host — server discovery is orchestrated by the caller (ShaPrint.UI PrintRelayClientService).");
            return false;
        }

        ct.ThrowIfCancellationRequested();

        // Reuse the exact PipeListener send flow for this target (same protocol, same failure semantics).
        string pipeName = WindowsVirtualPrinterManager.DerivePipeName(targetPrinter);
        var listener = new PipeListener(pipeName, serverIp, targetPrinter, targetPrinter);
        return await listener.SendToServerAsync(data, documentName);
    }
}