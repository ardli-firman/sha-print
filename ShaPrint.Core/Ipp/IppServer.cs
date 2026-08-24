using SharpIpp;
using SharpIpp.Exceptions;
using SharpIpp.Models;
using SharpIpp.Models.Requests;
using SharpIpp.Models.Responses;
using SharpIpp.Protocol;
using SharpIpp.Protocol.Models;

namespace ShaPrint.Core.Ipp;

/// <summary>
/// IPP print server implementation.
/// Deep module: one ProcessRequestAsync method handles the entire IPP lifecycle.
/// Supports multi-printer mode (extract from request) or single-printer mode (configured).
/// </summary>
public class IppServer : IIppServer
{
    private readonly ISpoolerAdapter _spooler;
    private readonly PrinterState _printerState;
    private readonly JobManager _jobManager;
    private readonly string? _configuredPrinterName;

    /// <summary>
    /// Multi-printer mode: printer name extracted from IPP request.
    /// </summary>
    public IppServer(ISpoolerAdapter spooler)
    {
        _spooler = spooler;
        _printerState = new PrinterState();
        _jobManager = new JobManager();
        _configuredPrinterName = null;
    }

    /// <summary>
    /// Single-printer mode: printer name configured at construction.
    /// </summary>
    public IppServer(ISpoolerAdapter spooler, string printerName)
    {
        _spooler = spooler;
        _printerState = new PrinterState();
        _jobManager = new JobManager();
        _configuredPrinterName = printerName;
    }

    public async Task ProcessRequestAsync(Stream inputStream, Stream outputStream, CancellationToken ct = default)
    {
        var ippProtocol = new SharpIppServer();

        try
        {
            // 1. Parse IPP request
            IIppRequest request = await ippProtocol.ReceiveRequestAsync(inputStream);

            // 2. Extract printer name from request or use configured one
            var printerName = _configuredPrinterName ?? ExtractPrinterNameFromRequest(request);

            // 3. Route to handler with printer context
            IIppResponse response = request switch
            {
                GetPrinterAttributesRequest r => await HandleGetPrinterAttributesAsync(r, printerName, ct),
                PrintJobRequest r => await HandlePrintJobAsync(r, printerName, ct),
                GetJobAttributesRequest r => HandleGetJobAttributes(r),
                GetJobsRequest r => HandleGetJobs(r),
                CancelJobRequest r => HandleCancelJob(r),
                ValidateJobRequest r => HandleValidateJob(r),
                _ => HandleUnsupportedOperation(request)
            };

            // 4. Serialize and send response
            IIppResponseMessage rawResponse = await ippProtocol.CreateRawResponseAsync(response);
            await ippProtocol.SendRawResponseAsync(rawResponse, outputStream);
        }
        catch (IppRequestException ex)
        {
            // Return IPP error response
            var responseVersion = ex.StatusCode == IppStatusCode.ServerErrorVersionNotSupported
                ? new IppVersion(1, 1)
                : ex.RequestMessage.Version;

            var errorResponse = new IppResponseMessage
            {
                RequestId = ex.RequestMessage.RequestId,
                Version = responseVersion,
                StatusCode = ex.StatusCode
            };
            errorResponse.OperationAttributes.Add([
                new IppAttribute(Tag.Charset, IppAttributeNames.AttributesCharset, "utf-8"),
                new IppAttribute(Tag.NaturalLanguage, IppAttributeNames.AttributesNaturalLanguage, "en")
            ]);
            await ippProtocol.SendRawResponseAsync(errorResponse, outputStream);
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // Request Handlers (private — not part of interface)
    // ═══════════════════════════════════════════════════════════════

    private async Task<GetPrinterAttributesResponse> HandleGetPrinterAttributesAsync(
        GetPrinterAttributesRequest request, string? printerName, CancellationToken ct)
    {
        var printers = await _spooler.GetPrintersAsync(ct);
        
        // Find the specific printer or use first one
        PrinterInfo? targetPrinter = null;
        if (printerName != null)
        {
            targetPrinter = printers.FirstOrDefault(p => 
                p.Name.Equals(printerName, StringComparison.OrdinalIgnoreCase));
        }
        targetPrinter ??= printers.FirstOrDefault();
        
        var resolvedName = targetPrinter?.Name ?? printerName ?? "ShaPrint";

        return new GetPrinterAttributesResponse
        {
            RequestId = request.RequestId,
            Version = request.Version,
            StatusCode = IppStatusCode.SuccessfulOk,
            PrinterAttributes = new()
            {
                PrinterName = resolvedName,
                PrinterState = _printerState.IsProcessing ? SharpIpp.Protocol.Models.PrinterState.Processing : SharpIpp.Protocol.Models.PrinterState.Idle,
                CharsetConfigured = "utf-8",
                CharsetSupported = ["utf-8"],
                NaturalLanguageConfigured = NaturalLanguage.En,
                GeneratedNaturalLanguageSupported = [NaturalLanguage.En],
                DocumentFormatDefault = "application/octet-stream",
                DocumentFormatSupported = GetSupportedFormats(),
                ColorSupported = true,
                CopiesSupported = new SharpIpp.Protocol.Models.Range(1, 99),
                OperationsSupported = [
                    IppOperation.PrintJob,
                    IppOperation.ValidateJob,
                    IppOperation.GetJobAttributes,
                    IppOperation.GetJobs,
                    IppOperation.CancelJob,
                    IppOperation.GetPrinterAttributes
                ],
                IppVersionsSupported = [
                    new IppVersion(1, 0),
                    new IppVersion(1, 1),
                    new IppVersion(2, 0)
                ],
                PrinterUriSupported = [new Uri($"ipp://localhost:631/printers/{resolvedName}")],
                QueuedJobCount = _jobManager.ActiveJobCount
            }
        };
    }

    private async Task<PrintJobResponse> HandlePrintJobAsync(PrintJobRequest request, string? printerName, CancellationToken ct)
    {
        // Use provided printer name or extract from request
        var targetPrinter = printerName ?? ExtractPrinterName(request.OperationAttributes?.PrinterUri);
        var documentName = request.OperationAttributes?.JobName ?? "Untitled";
        var documentData = ReadDocumentData(request.Document);
        var documentFormat = GetDocumentFormat(request);

        // Create job
        var job = _jobManager.CreateJob(targetPrinter, documentName);

        // Send to spooler (format travels with the document so the printer driver
        // gets the data it actually understands rather than an implicit raw blob)
        var result = await _spooler.PrintAsync(new PrintJob
        {
            PrinterName = targetPrinter,
            Data = documentData,
            DocumentName = documentName,
            DocumentFormat = documentFormat
        }, ct);

        if (result.Success)
        {
            job.MarkCompleted(result.JobId ?? 0);
            return new PrintJobResponse
            {
                RequestId = request.RequestId,
                Version = request.Version,
                StatusCode = IppStatusCode.SuccessfulOk,
                JobAttributes = new JobAttributes
                {
                    JobId = job.Id,
                    JobState = JobState.Completed,
                    JobStateReasons = [JobStateReason.None]
                }
            };
        }
        else
        {
            job.MarkFailed(result.ErrorMessage ?? "Unknown error");
            return new PrintJobResponse
            {
                RequestId = request.RequestId,
                Version = request.Version,
                StatusCode = IppStatusCode.ServerErrorInternalError,
                JobAttributes = new JobAttributes
                {
                    JobId = job.Id,
                    JobState = JobState.Aborted,
                    JobStateReasons = [JobStateReason.ProcessingToStopPoint]
                }
            };
        }
    }

    private GetJobAttributesResponse HandleGetJobAttributes(GetJobAttributesRequest request)
    {
        var jobId = GetJobIdFromRequest(request.OperationAttributes);
        if (!jobId.HasValue)
        {
            return new GetJobAttributesResponse
            {
                RequestId = request.RequestId,
                Version = request.Version,
                StatusCode = IppStatusCode.ClientErrorNotPossible,
                JobAttributes = new()
            };
        }

        var job = _jobManager.GetJob(jobId.Value);
        if (job == null)
        {
            return new GetJobAttributesResponse
            {
                RequestId = request.RequestId,
                Version = request.Version,
                StatusCode = IppStatusCode.ClientErrorNotFound,
                JobAttributes = new()
            };
        }

        var response = new GetJobAttributesResponse
        {
            RequestId = request.RequestId,
            Version = request.Version,
            StatusCode = IppStatusCode.SuccessfulOk,
            JobAttributes = new()
        };
        response.JobAttributes.JobId = job.Id;
        response.JobAttributes.JobState = job.State;
        response.JobAttributes.JobStateReasons = job.StateReasons.ToArray();
        return response;
    }

    private GetJobsResponse HandleGetJobs(GetJobsRequest request)
    {
        var jobs = _jobManager.GetAllJobs();

        return new GetJobsResponse
        {
            RequestId = request.RequestId,
            Version = request.Version,
            StatusCode = IppStatusCode.SuccessfulOk,
            JobsAttributes = jobs.Select(j => new JobDescriptionAttributes
            {
                JobId = j.Id,
                JobState = j.State,
                JobStateReasons = j.StateReasons.ToArray()
            }).ToArray()
        };
    }

    private CancelJobResponse HandleCancelJob(CancelJobRequest request)
    {
        var jobId = GetJobIdFromRequest(request.OperationAttributes);
        if (!jobId.HasValue)
        {
            return new CancelJobResponse
            {
                RequestId = request.RequestId,
                Version = request.Version,
                StatusCode = IppStatusCode.ClientErrorNotPossible
            };
        }

        var success = _jobManager.CancelJob(jobId.Value);

        return new CancelJobResponse
        {
            RequestId = request.RequestId,
            Version = request.Version,
            StatusCode = success ? IppStatusCode.SuccessfulOk : IppStatusCode.ClientErrorNotPossible
        };
    }

    private ValidateJobResponse HandleValidateJob(ValidateJobRequest request)
    {
        return new ValidateJobResponse
        {
            RequestId = request.RequestId,
            Version = request.Version,
            StatusCode = IppStatusCode.SuccessfulOk
        };
    }

    private IIppResponse HandleUnsupportedOperation(IIppRequest request)
    {
        return new ValidateJobResponse
        {
            RequestId = request.RequestId,
            Version = request.Version,
            StatusCode = IppStatusCode.ServerErrorOperationNotSupported
        };
    }

    // ═══════════════════════════════════════════════════════════════
    // Helpers
    // ═══════════════════════════════════════════════════════════════

    /// <summary>
    /// Extract printer name from IPP request (printer-uri attribute).
    /// </summary>
    private static string? ExtractPrinterNameFromRequest(IIppRequest request)
    {
        return request switch
        {
            GetPrinterAttributesRequest r => ExtractPrinterName(r.OperationAttributes?.PrinterUri),
            PrintJobRequest r => ExtractPrinterName(r.OperationAttributes?.PrinterUri),
            ValidateJobRequest r => ExtractPrinterName(r.OperationAttributes?.PrinterUri),
            _ => null
        };
    }

    private static int? GetJobIdFromRequest(object? operationAttributes)
    {
        // Try to extract job-id from various request types
        if (operationAttributes is JobOperationAttributes jobAttrs)
        {
            return jobAttrs.JobId;
        }
        return null;
    }

    private static string ExtractPrinterName(Uri? printerUri)
    {
        if (printerUri == null) return "Unknown";
        var segments = printerUri.Segments;
        return segments.Length > 0 ? segments.Last().TrimEnd('/') : "Unknown";
    }

    private static byte[] ReadDocumentData(Stream? document)
    {
        if (document == null) return Array.Empty<byte>();
        if (document is MemoryStream ms)
        {
            return ms.ToArray();
        }
        using var memoryStream = new MemoryStream();
        document.CopyTo(memoryStream);
        return memoryStream.ToArray();
    }

    /// <summary>
    /// MIME types this server can hand through to the Windows spooler.
    /// These mirror what a Microsoft IPP class driver client actually sends.
    /// </summary>
    private static readonly HashSet<string> _supportedFormats =
    [
        "application/pdf",
        "application/postscript",
        "image/jpeg",
        "image/pwg-raster",
        "image/urf",
        "text/plain",
        "application/octet-stream"
    ];

    /// <summary>
    /// Extract the client-declared document format. Returns <c>null</c> when the
    /// client omitted it or sent a format we don't advertise as supported.
    /// </summary>
    private static string? GetDocumentFormat(PrintJobRequest request)
    {
        var declared = request.OperationAttributes?.DocumentFormat?.Value;
        return declared != null && _supportedFormats.Contains(declared) ? declared : null;
    }

    /// <summary>
    /// Formats advertised in Get-Printer-Attributes so the client renders in a
    /// format this server can actually accept (fidelity depends on this being honest).
    /// </summary>
    private static string[] GetSupportedFormats() =>
        ["application/pdf", "image/pwg-raster", "image/urf", "application/octet-stream"];
}

/// <summary>
/// In-memory printer state tracking.
/// </summary>
internal class PrinterState
{
    public bool IsProcessing { get; set; }
    public List<string> StateReasons { get; set; } = [];
}

/// <summary>
/// In-memory job manager.
/// </summary>
internal class JobManager
{
    private readonly System.Collections.Concurrent.ConcurrentDictionary<int, IppJob> _jobs = new();
    private int _nextJobId;

    public int ActiveJobCount => _jobs.Values.Count(j =>
        j.State == JobState.Pending || j.State == JobState.Processing);

    public IppJob CreateJob(string printerName, string documentName)
    {
        int id = Interlocked.Increment(ref _nextJobId);
        var job = new IppJob
        {
            Id = id,
            PrinterName = printerName,
            DocumentName = documentName,
            State = JobState.Processing,
            StateReasons = [JobStateReason.None],
            CreatedAt = DateTime.UtcNow
        };
        if (!_jobs.TryAdd(id, job))
        {
            throw new InvalidOperationException($"IPP job ID collision: {id}.");
        }

        return job;
    }

    public IppJob? GetJob(int jobId)
    {
        return _jobs.TryGetValue(jobId, out var job) ? job : null;
    }

    public IReadOnlyList<IppJob> GetAllJobs()
    {
        return _jobs.Values.OrderBy(job => job.Id).ToArray();
    }

    public bool CancelJob(int jobId)
    {
        if (!_jobs.TryGetValue(jobId, out var job))
            return false;
        if (job.State == JobState.Completed || job.State == JobState.Canceled)
            return false;

        job.State = JobState.Canceled;
        job.StateReasons = [JobStateReason.None]; // Use None instead of CanceledByUser
        return true;
    }
}

/// <summary>
/// Internal job representation.
/// </summary>
internal class IppJob
{
    public int Id { get; init; }
    public string PrinterName { get; init; } = string.Empty;
    public string DocumentName { get; init; } = string.Empty;
    public JobState State { get; set; }
    public List<JobStateReason> StateReasons { get; set; } = [];
    public DateTime CreatedAt { get; init; }
    public int? SpoolerJobId { get; set; }

    public void MarkCompleted(int spoolerJobId)
    {
        State = JobState.Completed;
        StateReasons = [JobStateReason.None];
        SpoolerJobId = spoolerJobId;
    }

    public void MarkFailed(string error)
    {
        State = JobState.Aborted;
        StateReasons = [JobStateReason.ProcessingToStopPoint];
    }
}
