namespace ShaPrint.Core.Ipp;

/// <summary>
/// Internal seam: abstracts print spooler for testability.
/// Production: writes to Windows spooler via Win32 API
/// Test: in-memory fake that captures print data
/// </summary>
public interface ISpoolerAdapter
{
    /// <summary>
    /// Send print data to the spooler for the specified printer.
    /// Returns success/failure with optional error message.
    /// </summary>
    Task<SpoolerResult> PrintAsync(PrintJob job, CancellationToken ct);

    /// <summary>
    /// Get list of available printers from the spooler.
    /// </summary>
    Task<IReadOnlyList<PrinterInfo>> GetPrintersAsync(CancellationToken ct);
}

/// <summary>
/// A resolved print job: the document bytes plus everything the spooler needs
/// to hand it to a printer driver correctly. Carries format together with data
/// so the adapter never drops what the client actually rendered.
/// </summary>
public record PrintJob
{
    public required string PrinterName { get; init; }
    public required byte[] Data { get; init; }
    public string DocumentName { get; init; } = "Untitled";

    /// <summary>
    /// MIME type of the document (e.g. image/pwg-raster, image/urf, application/pdf).
    /// Set by the server from the IPP request; empty when the client supplied none.
    /// </summary>
    public string? DocumentFormat { get; init; }
}

/// <summary>
/// Result of a spooler print operation.
/// </summary>
public record SpoolerResult
{
    public bool Success { get; init; }
    public string? ErrorMessage { get; init; }
    public int? JobId { get; init; }

    public static SpoolerResult Ok(int jobId) => new() { Success = true, JobId = jobId };
    public static SpoolerResult Fail(string error) => new() { Success = false, ErrorMessage = error };
}

/// <summary>
/// Printer information from the spooler.
/// </summary>
public record PrinterInfo
{
    public string Name { get; init; } = string.Empty;
    public string DriverName { get; init; } = string.Empty;
    public bool IsOnline { get; init; } = true;
}
