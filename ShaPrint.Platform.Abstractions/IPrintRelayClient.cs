namespace ShaPrint.Platform.Abstractions;

public interface IPrintRelayClient
{
    Task<bool> SendAsync(string targetPrinter, byte[] data, string documentName,
                         string? hostOverride = null, CancellationToken ct = default);
}