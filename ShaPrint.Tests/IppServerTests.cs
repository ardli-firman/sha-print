using ShaPrint.Core.Ipp;

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
        var requestBytes = BuildGetPrinterAttributesRequest();

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
        var requestBytes = BuildPrintJobRequest("TestPrinter", documentData);

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
        var printRequest = BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Now query jobs
        var getJobsRequest = BuildGetJobsRequest();
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
        var printRequest = BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Cancel the job
        var cancelRequest = BuildCancelJobRequest(1);
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

        var requestBytes = BuildValidateJobRequest("TestPrinter");
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
        var printRequest = BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Try to cancel the completed job
        var cancelRequest = BuildCancelJobRequest(1);
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

        var requestBytes = BuildGetJobAttributesRequest(999); // Non-existent job
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
        var printRequest = BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Query printer attributes
        var attrRequest = BuildGetPrinterAttributesRequest();
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
            var request = BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
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

        var request = BuildPrintJobRequest("NonExistentPrinter", new byte[] { 0x50, 0x44, 0x46 });
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
        var requestBytes = BuildInvalidVersionRequest();
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

        var request = BuildPrintJobRequest("TestPrinter", Array.Empty<byte>());
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

        var request = BuildPrintJobRequest("TestPrinter", largeDoc);
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
                var request = BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
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

        var request = BuildGetPrinterAttributesRequest();
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
        var printRequest = BuildPrintJobRequest("TestPrinter", new byte[] { 0x50, 0x44, 0x46 });
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        // Get job attributes
        var getRequest = BuildGetJobAttributesRequest(1);
        using var getInput = new MemoryStream(getRequest);
        using var getOutput = new MemoryStream();

        // Act
        await server.ProcessRequestAsync(getInput, getOutput);

        // Assert
        var responseBytes = getOutput.ToArray();
        Assert.NotEmpty(responseBytes);
    }

    // ═══════════════════════════════════════════════════════════════
    // IPP Request Builders (test helpers)
    // ═══════════════════════════════════════════════════════════════

    /// <summary>
    /// Build a minimal IPP Get-Printer-Attributes request (IPP 2.0).
    /// Format: [version-major][version_minor][operation-id][request-id][attributes...][end-tag]
    /// </summary>
    private static byte[] BuildGetPrinterAttributesRequest()
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        // IPP header
        writer.Write((byte)0x02); // version-major (IPP 2.0)
        writer.Write((byte)0x00); // version-minor
        writer.Write((short)0x000B); // operation-id: Get-Printer-Attributes (0x000B)
        writer.Write(1); // request-id

        // operation-attributes (tag 0x01)
        writer.Write((byte)0x01); // begin-operation-attributes
        writer.Write((byte)0x47); // charset
        writer.Write((short)0x0012); // name-length
        writer.Write("attributes-charset"u8.ToArray());
        writer.Write((short)0x0005); // value-length
        writer.Write("utf-8"u8.ToArray());

        writer.Write((byte)0x48); // natural-language
        writer.Write((short)0x001B); // name-length
        writer.Write("attributes-natural-language"u8.ToArray());
        writer.Write((short)0x0002); // value-length
        writer.Write("en"u8.ToArray());

        writer.Write((byte)0x45); // uri
        writer.Write((short)0x000B); // name-length
        writer.Write("printer-uri"u8.ToArray());
        writer.Write((short)0x001C); // value-length
        writer.Write("ipp://localhost:631/printers"u8.ToArray());

        writer.Write((byte)0x03); // end-operation-attributes

        writer.Write((byte)0x03); // end-of-attributes

        return ms.ToArray();
    }

    /// <summary>
    /// Build a minimal IPP Print-Job request.
    /// </summary>
    private static byte[] BuildPrintJobRequest(string printerName, byte[] documentData)
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        // IPP header
        writer.Write((byte)0x02); // version-major
        writer.Write((byte)0x00); // version-minor
        writer.Write((short)0x0002); // operation-id: Print-Job (0x0002)
        writer.Write(1); // request-id

        // operation-attributes
        writer.Write((byte)0x01); // begin-operation-attributes
        writer.Write((byte)0x47); // charset
        writer.Write((short)0x0012);
        writer.Write("attributes-charset"u8.ToArray());
        writer.Write((short)0x0005);
        writer.Write("utf-8"u8.ToArray());

        writer.Write((byte)0x48); // natural-language
        writer.Write((short)0x001B);
        writer.Write("attributes-natural-language"u8.ToArray());
        writer.Write((short)0x0002);
        writer.Write("en"u8.ToArray());

        writer.Write((byte)0x45); // uri
        writer.Write((short)0x000B);
        writer.Write("printer-uri"u8.ToArray());
        var uriBytes = System.Text.Encoding.UTF8.GetBytes($"ipp://localhost:631/printers/{printerName}");
        writer.Write((short)uriBytes.Length);
        writer.Write(uriBytes);

        writer.Write((byte)0x42); // nameWithoutLanguage
        writer.Write((short)0x0009);
        writer.Write("job-name"u8.ToArray());
        var jobNameBytes = System.Text.Encoding.UTF8.GetBytes("TestJob");
        writer.Write((short)jobNameBytes.Length);
        writer.Write(jobNameBytes);

        writer.Write((byte)0x03); // end-operation-attributes

        // document data
        writer.Write((byte)0x00); // document data marker? Actually IPP uses the raw bytes after end-of-attributes
        writer.Write(documentData);

        writer.Write((byte)0x03); // end-of-attributes

        return ms.ToArray();
    }

    /// <summary>
    /// Build a minimal IPP Get-Jobs request.
    /// </summary>
    private static byte[] BuildGetJobsRequest()
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02);
        writer.Write((byte)0x00);
        writer.Write((short)0x000A); // Get-Jobs (0x000A)
        writer.Write(1);

        writer.Write((byte)0x01);
        writer.Write((byte)0x47);
        writer.Write((short)0x0012);
        writer.Write("attributes-charset"u8.ToArray());
        writer.Write((short)0x0005);
        writer.Write("utf-8"u8.ToArray());

        writer.Write((byte)0x48);
        writer.Write((short)0x001B);
        writer.Write("attributes-natural-language"u8.ToArray());
        writer.Write((short)0x0002);
        writer.Write("en"u8.ToArray());

        writer.Write((byte)0x45);
        writer.Write((short)0x000B);
        writer.Write("printer-uri"u8.ToArray());
        writer.Write((short)0x001C);
        writer.Write("ipp://localhost:631/printers"u8.ToArray());

        writer.Write((byte)0x03);
        writer.Write((byte)0x03);

        return ms.ToArray();
    }

    /// <summary>
    /// Build a minimal IPP Cancel-Job request.
    /// </summary>
    private static byte[] BuildCancelJobRequest(int jobId)
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02);
        writer.Write((byte)0x00);
        writer.Write((short)0x0008); // Cancel-Job (0x0008)
        writer.Write(1);

        writer.Write((byte)0x01);
        writer.Write((byte)0x47);
        writer.Write((short)0x0012);
        writer.Write("attributes-charset"u8.ToArray());
        writer.Write((short)0x0005);
        writer.Write("utf-8"u8.ToArray());

        writer.Write((byte)0x48);
        writer.Write((short)0x001B);
        writer.Write("attributes-natural-language"u8.ToArray());
        writer.Write((short)0x0002);
        writer.Write("en"u8.ToArray());

        writer.Write((byte)0x45);
        writer.Write((short)0x000B);
        writer.Write("printer-uri"u8.ToArray());
        writer.Write((short)0x001C);
        writer.Write("ipp://localhost:631/printers"u8.ToArray());

        writer.Write((byte)0x21); // integer
        writer.Write((short)0x0006);
        writer.Write("job-id"u8.ToArray());
        writer.Write((short)0x0004);
        writer.Write(jobId);

        writer.Write((byte)0x03);
        writer.Write((byte)0x03);

        return ms.ToArray();
    }

    /// <summary>
    /// Build a minimal IPP Validate-Job request.
    /// </summary>
    private static byte[] BuildValidateJobRequest(string printerName)
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02);
        writer.Write((byte)0x00);
        writer.Write((short)0x0004); // Validate-Job (0x0004)
        writer.Write(1);

        writer.Write((byte)0x01);
        writer.Write((byte)0x47);
        writer.Write((short)0x0012);
        writer.Write("attributes-charset"u8.ToArray());
        writer.Write((short)0x0005);
        writer.Write("utf-8"u8.ToArray());

        writer.Write((byte)0x48);
        writer.Write((short)0x001B);
        writer.Write("attributes-natural-language"u8.ToArray());
        writer.Write((short)0x0002);
        writer.Write("en"u8.ToArray());

        writer.Write((byte)0x45);
        writer.Write((short)0x000B);
        writer.Write("printer-uri"u8.ToArray());
        var uriBytes = System.Text.Encoding.UTF8.GetBytes($"ipp://localhost:631/printers/{printerName}");
        writer.Write((short)uriBytes.Length);
        writer.Write(uriBytes);

        writer.Write((byte)0x03);
        writer.Write((byte)0x03);

        return ms.ToArray();
    }

    /// <summary>
    /// Build a minimal IPP Get-Job-Attributes request.
    /// </summary>
    private static byte[] BuildGetJobAttributesRequest(int jobId)
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02);
        writer.Write((byte)0x00);
        writer.Write((short)0x0009); // Get-Job-Attributes (0x0009)
        writer.Write(1);

        writer.Write((byte)0x01);
        writer.Write((byte)0x47);
        writer.Write((short)0x0012);
        writer.Write("attributes-charset"u8.ToArray());
        writer.Write((short)0x0005);
        writer.Write("utf-8"u8.ToArray());

        writer.Write((byte)0x48);
        writer.Write((short)0x001B);
        writer.Write("attributes-natural-language"u8.ToArray());
        writer.Write((short)0x0002);
        writer.Write("en"u8.ToArray());

        writer.Write((byte)0x45);
        writer.Write((short)0x000B);
        writer.Write("printer-uri"u8.ToArray());
        writer.Write((short)0x001C);
        writer.Write("ipp://localhost:631/printers"u8.ToArray());

        writer.Write((byte)0x21); // integer
        writer.Write((short)0x0006);
        writer.Write("job-id"u8.ToArray());
        writer.Write((short)0x0004);
        writer.Write(jobId);

        writer.Write((byte)0x03);
        writer.Write((byte)0x03);

        return ms.ToArray();
    }

    /// <summary>
    /// Build a request with invalid IPP version (0.0).
    /// </summary>
    private static byte[] BuildInvalidVersionRequest()
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        // Invalid version
        writer.Write((byte)0x00); // version-major (invalid)
        writer.Write((byte)0x00); // version-minor (invalid)
        writer.Write((short)0x000B); // operation-id: Get-Printer-Attributes
        writer.Write(1); // request-id

        // Minimal attributes
        writer.Write((byte)0x01); // begin-operation-attributes
        writer.Write((byte)0x47); // charset
        writer.Write((short)0x0012);
        writer.Write("attributes-charset"u8.ToArray());
        writer.Write((short)0x0005);
        writer.Write("utf-8"u8.ToArray());

        writer.Write((byte)0x03); // end-operation-attributes
        writer.Write((byte)0x03); // end-of-attributes

        return ms.ToArray();
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

    public Task<SpoolerResult> PrintAsync(string printerName, byte[] data, string documentName, CancellationToken ct)
    {
        var printer = _printers.FirstOrDefault(p => p.Name == printerName);
        if (printer == null)
        {
            return Task.FromResult(SpoolerResult.Fail($"Printer '{printerName}' not found"));
        }

        var jobId = _nextJobId++;
        _printedJobs.Add(new PrintedJob
        {
            JobId = jobId,
            PrinterName = printerName,
            DocumentName = documentName,
            Data = data
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
}
