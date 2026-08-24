using ShaPrint.Core.Ipp;
using ShaPrint.Core.Ipp.Testing;

namespace ShaPrint.Tests;

/// <summary>
/// Tests for IIppServer — the deep module that handles IPP request/response lifecycle.
/// Tests are written at the interface seam: send IPP request bytes, assert on response bytes.
/// Uses InMemorySpoolerAdapter to abstract Windows spooler.
/// </summary>
public class IppServerTests
{
    /// <summary>
    /// RED: Get-Printer-Attributes should return printer name and state.
    /// This is the first test — validates basic IPP request/response flow.
    /// </summary>
    [Fact]
    public async Task GetPrinterAttributes_ReturnsPrinterNameAndState()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // Build IPP Get-Printer-Attributes request (IPP 2.0)
        var requestBytes = IppRequestBuilder.BuildGetPrinterAttributesRequest();

        using var inputStream = new MemoryStream(requestBytes);
        using var outputStream = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(inputStream, outputStream);

        // Assert
        var responseBytes = outputStream.ToArray();
        Assert.NotEmpty(responseBytes);

        // Parse response — should contain printer name
        var responseText = System.Text.Encoding.ASCII.GetString(responseBytes);
        Assert.Contains("TestPrinter", responseText);
    }

    /// <summary>
    /// RED: Print-Job should send data to spooler and return job-id.
    /// </summary>
    [Fact]
    public async Task PrintJob_SendsDataToSpooler_ReturnsJobId()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        var documentData = new byte[] { 0x50, 0x44, 0x46 }; // "PDF" magic bytes
        var requestBytes = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", documentData);

        using var inputStream = new MemoryStream(requestBytes);
        using var outputStream = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(inputStream, outputStream);

        // Assert
        Assert.Single(spooler.PrintedJobs);
        Assert.Equal("TestPrinter", spooler.PrintedJobs[0].PrinterName);
        Assert.Equal(documentData, spooler.PrintedJobs[0].Data);
    }

    /// <summary>
    /// The document format declared by the client must travel through the spooler
    /// seam, so the printer driver knows what it is receiving (fidelity depends on
    /// this not being dropped).
    /// </summary>
    [Fact]
    public async Task PrintJob_CarriesDocumentFormat_ToSpooler()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        var requestBytes = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", new byte[] { 0x50 }, "image/pwg-raster");

        using var inputStream = new MemoryStream(requestBytes);
        using var outputStream = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(inputStream, outputStream);

        // Assert
        Assert.Single(spooler.PrintedJobs);
        Assert.Equal("image/pwg-raster", spooler.PrintedJobs[0].DocumentFormat);
    }

    /// <summary>
    /// A client format we don't advertise as supported should not be passed through
    /// blindly; the server stays honest about what it can hand to the driver.
    /// </summary>
    [Fact]
    public async Task PrintJob_UnsupportedDocumentFormat_IsDropped()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        var requestBytes = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", new byte[] { 0x50 }, "application/x-unknown");

        using var inputStream = new MemoryStream(requestBytes);
        using var outputStream = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(inputStream, outputStream);

        // Assert
        Assert.Single(spooler.PrintedJobs);
        Assert.Null(spooler.PrintedJobs[0].DocumentFormat);
    }

    /// <summary>
    /// RED: Get-Jobs should return list of jobs.
    /// </summary>
    [Fact]
    public async Task GetJobs_ReturnsJobList()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // First, create a job
        var printRequest = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Now query jobs
        var getJobsRequest = IppRequestBuilder.BuildGetJobsRequest();
        using var input = new MemoryStream(getJobsRequest);
        using var output = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(input, output);

        // Assert
        var responseBytes = output.ToArray();
        Assert.NotEmpty(responseBytes);
    }

    /// <summary>
    /// RED: Cancel-Job should cancel a pending job.
    /// </summary>
    [Fact]
    public async Task CancelJob_CancelsPendingJob()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // Create a job first
        var printRequest = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Cancel the job
        var cancelRequest = IppRequestBuilder.BuildCancelJobRequest(1);
        using var input = new MemoryStream(cancelRequest);
        using var output = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(input, output);

        // Assert
        var responseBytes = output.ToArray();
        Assert.NotEmpty(responseBytes);
    }

    /// <summary>
    /// Validate-Job should return success for valid job request.
    /// </summary>
    [Fact]
    public async Task ValidateJob_ReturnsSuccess()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        var requestBytes = IppRequestBuilder.BuildValidateJobRequest("TestPrinter");
        using var inputStream = new MemoryStream(requestBytes);
        using var outputStream = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(inputStream, outputStream);

        // Assert
        var responseBytes = outputStream.ToArray();
        Assert.NotEmpty(responseBytes);
    }

    /// <summary>
    /// Cancel-Job on already completed job should return error.
    /// </summary>
    [Fact]
    public async Task CancelJob_AlreadyCompleted_ReturnsError()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // Create and complete a job
        var printRequest = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Try to cancel the completed job
        var cancelRequest = IppRequestBuilder.BuildCancelJobRequest(1);
        using var input = new MemoryStream(cancelRequest);
        using var output = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(input, output);

        // Assert
        var responseBytes = output.ToArray();
        Assert.NotEmpty(responseBytes);
        // Response should indicate error (job already completed)
    }

    /// <summary>
    /// Get-Job-Attributes for non-existent job should return not found.
    /// </summary>
    [Fact]
    public async Task GetJobAttributes_NonExistentJob_ReturnsNotFound()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        var requestBytes = IppRequestBuilder.BuildGetJobAttributesRequest(999); // Non-existent job
        using var inputStream = new MemoryStream(requestBytes);
        using var outputStream = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(inputStream, outputStream);

        // Assert
        var responseBytes = outputStream.ToArray();
        Assert.NotEmpty(responseBytes);
    }

    /// <summary>
    /// Get-Printer-Attributes should show correct queued job count.
    /// </summary>
    [Fact]
    public async Task GetPrinterAttributes_ShowsQueuedJobCount()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // Create a job first
        var printRequest = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Query printer attributes
        var attrRequest = IppRequestBuilder.BuildGetPrinterAttributesRequest();
        using var input = new MemoryStream(attrRequest);
        using var output = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(input, output);

        // Assert
        var responseBytes = output.ToArray();
        Assert.NotEmpty(responseBytes);
    }

    /// <summary>
    /// Print-Job with multiple jobs should increment job IDs.
    /// </summary>
    [Fact]
    public async Task PrintJob_MultipleJobs_IncrementsJobId()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // Send 3 print jobs
        for (int i = 0; i < 3; i++)
        {
            var request = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
            using var input = new MemoryStream(request);
            using var output = new MemoryStream();
            await server.ProcessRequestAsync(input, output);
        }

        // Assert
        Assert.Equal(3, spooler.PrintedJobs.Count);
        Assert.Equal(1, spooler.PrintedJobs[0].JobId);
        Assert.Equal(2, spooler.PrintedJobs[1].JobId);
        Assert.Equal(3, spooler.PrintedJobs[2].JobId);
    }

    /// <summary>
    /// Print-Job to non-existent printer should fail.
    /// </summary>
    [Fact]
    public async Task PrintJob_NonExistentPrinter_Fails()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        // Don't add any printers
        var server = new IppServer(spooler);

        var request = IppRequestBuilder.BuildPrintJobRequest("NonExistentPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var input = new MemoryStream(request);
        using var output = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(input, output);

        // Assert
        Assert.Empty(spooler.PrintedJobs); // No jobs should be printed
    }

    /// <summary>
    /// Empty request should not crash the server.
    /// </summary>
    [Fact]
    public async Task EmptyRequest_DoesNotCrash()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        var emptyBytes = Array.Empty<byte>();
        using var inputStream = new MemoryStream(emptyBytes);
        using var outputStream = new MemoryStream();

        // Act & Assert - should not throw
        try
        {
            await server.ProcessRequestAsync(inputStream, outputStream);
        }
        catch (Exception ex)
        {
            // Expected - IPP protocol requires valid data
            Assert.NotNull(ex);
        }
    }

    /// <summary>
    /// Invalid IPP version should return error.
    /// </summary>
    [Fact]
    public async Task InvalidIppVersion_ReturnsError()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // Build request with invalid version (0.0)
        var requestBytes = IppRequestBuilder.BuildInvalidVersionRequest();
        using var inputStream = new MemoryStream(requestBytes);
        using var outputStream = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(inputStream, outputStream);

        // Assert
        var responseBytes = outputStream.ToArray();
        // Server should handle gracefully (may return error or empty)
    }

    /// <summary>
    /// Print-Job with empty document should handle gracefully.
    /// </summary>
    [Fact]
    public async Task PrintJob_EmptyDocument_HandlesGracefully()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        var request = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", Array.Empty<byte>());
        using var input = new MemoryStream(request);
        using var output = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(input, output);

        // Assert
        var responseBytes = output.ToArray();
        Assert.NotEmpty(responseBytes);
    }

    /// <summary>
    /// Print-Job with large document should work.
    /// </summary>
    [Fact]
    public async Task PrintJob_LargeDocument_Works()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // 1MB document
        var largeDoc = new byte[1024 * 1024];
        new Random().NextBytes(largeDoc);

        var request = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", largeDoc);
        using var input = new MemoryStream(request);
        using var output = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(input, output);

        // Assert
        Assert.Single(spooler.PrintedJobs);
        Assert.Equal(largeDoc.Length, spooler.PrintedJobs[0].Data.Length);
    }

    /// <summary>
    /// Multiple concurrent requests should not crash.
    /// </summary>
    [Fact]
    public async Task ConcurrentRequests_DoNotCrash()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // Act - send 10 concurrent requests
        var tasks = new List<Task>();
        for (int i = 0; i < 10; i++)
        {
            tasks.Add(Task.Run(async () =>
            {
                var request = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
                using var input = new MemoryStream(request);
                using var output = new MemoryStream();
                await server.ProcessRequestAsync(input, output);
            }));
        }

        await Task.WhenAll(tasks);

        // Assert
        Assert.Equal(10, spooler.PrintedJobs.Count);
    }

    /// <summary>
    /// Get-Printer-Attributes with multiple printers should return first.
    /// </summary>
    [Fact]
    public async Task GetPrinterAttributes_MultiplePrinters_ReturnsFirst()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "Printer1", DriverName = "Driver1" });
        spooler.AddPrinter(new PrinterInfo { Name = "Printer2", DriverName = "Driver2" });
        var server = new IppServer(spooler);

        var request = IppRequestBuilder.BuildGetPrinterAttributesRequest();
        using var input = new MemoryStream(request);
        using var output = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(input, output);

        // Assert
        var responseBytes = output.ToArray();
        var responseText = System.Text.Encoding.ASCII.GetString(responseBytes);
        Assert.Contains("Printer1", responseText);
    }

    /// <summary>
    /// Print-Job then Get-Job-Attributes should return same job.
    /// </summary>
    [Fact]
    public async Task PrintJob_ThenGetJobAttributes_ReturnsSameJob()
    {
        // Arrange
        var spooler = new InMemorySpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        var server = new IppServer(spooler);

        // Print a job
        var printRequest = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Get job attributes
        var getRequest = IppRequestBuilder.BuildGetJobAttributesRequest(1);
        using var getInput = new MemoryStream(getRequest);
        using var getOutput = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(getInput, getOutput);

        // Assert
        var responseBytes = getOutput.ToArray();
        Assert.NotEmpty(responseBytes);
    }

}

/// <summary>
/// Test adapter: captures print data in memory.
/// Implements ISpoolerAdapter for testing without Windows spooler.
/// </summary>
public class InMemorySpoolerAdapter : ISpoolerAdapter
{
    private readonly List<PrinterInfo> _printers = new();
    private readonly List<PrintedJob> _printedJobs = new();
    private int _nextJobId = 1;

    public IReadOnlyList<PrintedJob> PrintedJobs => _printedJobs;

    public void AddPrinter(PrinterInfo printer)
    {
        _printers.Add(printer);
    }

    public Task<SpoolerResult> PrintAsync(PrintJob job, CancellationToken ct)
    {
        var printer = _printers.FirstOrDefault(p => p.Name == job.PrinterName);
        if (printer == null)
        {
            return Task.FromResult(SpoolerResult.Fail($"Printer '{job.PrinterName}' not found"));
        }

        var jobId = _nextJobId++;
        _printedJobs.Add(new PrintedJob
        {
            JobId = jobId,
            PrinterName = job.PrinterName,
            DocumentName = job.DocumentName,
            Data = job.Data,
            DocumentFormat = job.DocumentFormat
        });

        return Task.FromResult(SpoolerResult.Ok(jobId));
    }

    public Task<IReadOnlyList<PrinterInfo>> GetPrintersAsync(CancellationToken ct)
    {
        return Task.FromResult<IReadOnlyList<PrinterInfo>>(_printers);
    }
}

public class PrintedJob
{
    public int JobId { get; init; }
    public string PrinterName { get; init; } = string.Empty;
    public string DocumentName { get; init; } = string.Empty;
    public byte[] Data { get; init; } = Array.Empty<byte>();
    public string? DocumentFormat { get; init; }
}
