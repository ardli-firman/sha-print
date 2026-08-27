using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Core;
using ShaPrint.Platform.Abstractions;
using ShaPrint.Platform.Unix;

namespace ShaPrint.UI.Cli;

/// <summary>Parsed arguments for the <c>send</c> verb. <see cref="HostOverride"/> is null when
/// the target server should be discovered on the LAN instead of pinned explicitly.</summary>
public sealed record SendOptions(string Printer, string FilePath, string? HostOverride);

/// <summary>
/// Executes the <c>send</c> verb: reads the spool file, resolves an
/// <see cref="IPrintRelayClient"/> and relays the bytes to the ShaPrint server (Task 8).
///
/// Resolve strategy (chosen option (a) from the task report):
/// <list type="bullet">
///   <item>Resolve <see cref="IPrintRelayClient"/> from the CLI service provider first.</item>
///   <item>When the platform switch did not register one — the plain <c>net8.0</c> build
///   running on Windows compiles <c>AddPlatformWindows</c> with an empty body
///   (<c>#if WINDOWS</c>), so no relay exists there — fall back to constructing
///   <see cref="UnixPrintRelayClient"/> directly. It is pure .NET (no OS-specific API), does
///   its own inline UDP discovery + HMAC verification when no host is given, and therefore
///   works on every OS. This keeps the CLI functional in every scenario instead of surfacing
///   an obscure DI resolve error.</item>
/// </list>
///
/// Why <see cref="Services.PrintRelayClientService"/> is NOT reused (task report): that shared
/// orchestration performs the richer discovery sweep (unicast per-interface, for AP-isolation
/// networks) and is designed for the interactive UI path. The CLI's contract — the plan's
/// <c>IPrintRelayClient + DiscoveryClient + PrintJobPayload</c> flow — is fully satisfied by
/// <see cref="UnixPrintRelayClient"/> alone (it performs discovery inline, as documented in
/// Task 7: "this fallback only runs for direct use"). Going straight to
/// <see cref="IPrintRelayClient"/> keeps the CLI host minimal and avoids forcing the full
/// discovery wiring onto it.
///
/// Returns <see cref="CliDispatcher.ExitSuccess"/> (0), <see cref="CliDispatcher.ExitParseOrIoError"/>
/// (1, file IO error) or <see cref="CliDispatcher.ExitSendFailed"/> (2, the send attempt failed).
/// </summary>
public static class SendCommand
{
    public static async Task<int> ExecuteAsync(IServiceProvider services, SendOptions options, CancellationToken ct = default)
    {
        byte[] data;
        try
        {
            data = await File.ReadAllBytesAsync(options.FilePath, ct);
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            Console.Error.WriteLine($"shaprint send: cannot read '{options.FilePath}': {ex.Message}");
            return CliDispatcher.ExitParseOrIoError;
        }

        if (data.Length == 0)
        {
            Console.Error.WriteLine($"shaprint send: '{options.FilePath}' is empty — refusing to send an empty job.");
            return CliDispatcher.ExitParseOrIoError;
        }

        // DI first; fall back to the self-sufficient relay when the platform switch did not
        // register one (net8.0-on-Windows scenario, see class docs). Never a confusing
        // "no service registered" error — the CLI must parse & attempt the send everywhere.
        var relay = services.GetService<IPrintRelayClient>() ?? new UnixPrintRelayClient();

        AppLogger.Log($"shaprint send: relaying '{options.FilePath}' ({data.Length} bytes) to printer '{options.Printer}'" +
                      (string.IsNullOrWhiteSpace(options.HostOverride)
                          ? " — discovering server on the LAN…"
                          : $" — host {options.HostOverride}"));

        bool sent = await relay.SendAsync(
            options.Printer,
            data,
            Path.GetFileName(options.FilePath),
            options.HostOverride,
            ct);

        if (sent)
        {
            AppLogger.Log($"shaprint send: job for printer '{options.Printer}' accepted by the server.");
            return CliDispatcher.ExitSuccess;
        }

        Console.Error.WriteLine($"shaprint send: failed to send job to printer '{options.Printer}' (see relay log above).");
        return CliDispatcher.ExitSendFailed;
    }
}
