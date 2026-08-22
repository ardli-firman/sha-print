namespace ShaPrint.Core.Ipp;

/// <summary>
/// Deep module: IPP print server.
/// One method handles the entire IPP request/response lifecycle.
/// Callers don't need to know about IPP protocol, job management, or spooler details.
/// </summary>
public interface IIppServer
{
    /// <summary>
    /// Process an IPP request from input stream and write response to output stream.
    /// Handles: protocol parsing, request routing, job management, printer state, spooler integration.
    /// </summary>
    Task ProcessRequestAsync(Stream inputStream, Stream outputStream, CancellationToken ct = default);
}
