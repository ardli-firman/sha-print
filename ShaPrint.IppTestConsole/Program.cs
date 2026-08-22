using ShaPrint.Core.Ipp;
using ShaPrint.Core.Ipp.Testing;

namespace ShaPrint.IppTestConsole;

/// <summary>
/// Test console app for IPP server.
/// Can run on Linux to verify IPP request/response flow.
/// </summary>
class Program
{
    static async Task Main(string[] args)
    {
        Console.WriteLine("=== ShaPrint IPP Test Console ===");
        Console.WriteLine();

        // Create in-memory spooler adapter
        var spooler = new TestSpoolerAdapter();
        spooler.AddPrinter(new PrinterInfo { Name = "TestPrinter", DriverName = "Generic Driver" });
        spooler.AddPrinter(new PrinterInfo { Name = "OfficePrinter", DriverName = "HP LaserJet" });

        // Create IPP server
        var server = new IppServer(spooler);

        Console.WriteLine("IPP Server created with 2 printers:");
        Console.WriteLine("  - TestPrinter (Generic Driver)");
        Console.WriteLine("  - OfficePrinter (HP LaserJet)");
        Console.WriteLine();

        // Test 1: Get-Printer-Attributes
        Console.WriteLine("Test 1: Get-Printer-Attributes");
        await TestGetPrinterAttributes(server);
        Console.WriteLine();

        // Test 2: Print-Job
        Console.WriteLine("Test 2: Print-Job");
        await TestPrintJob(server, spooler);
        Console.WriteLine();

        // Test 3: Get-Jobs
        Console.WriteLine("Test 3: Get-Jobs");
        await TestGetJobs(server);
        Console.WriteLine();

        // Test 4: Cancel-Job
        Console.WriteLine("Test 4: Cancel-Job");
        await TestCancelJob(server, spooler);
        Console.WriteLine();

        // Test 5: Validate-Job
        Console.WriteLine("Test 5: Validate-Job");
        await TestValidateJob(server);
        Console.WriteLine();

        Console.WriteLine("=== All tests completed ===");
    }

    static async Task TestGetPrinterAttributes(IppServer server)
    {
        var request = IppRequestBuilder.BuildGetPrinterAttributesRequest();
        Console.WriteLine($"  Request: {request.Length} bytes");
        Console.WriteLine($"  Hex: {BitConverter.ToString(request[..Math.Min(20, request.Length)])}...");

        using var input = new MemoryStream(request);
        using var output = new MemoryStream();

        try
        {
            await server.ProcessRequestAsync(input, output);

            var response = output.ToArray();
            Console.WriteLine($"  Response: {response.Length} bytes");
            Console.WriteLine($"  Hex: {BitConverter.ToString(response[..Math.Min(20, response.Length)])}...");

            // Check if response contains printer name
            var text = System.Text.Encoding.ASCII.GetString(response);
            if (text.Contains("TestPrinter"))
                Console.WriteLine("  ✓ Contains printer name 'TestPrinter'");
            else
                Console.WriteLine("  ✗ Missing printer name (response may be error)");
        }
        catch (Exception ex)
        {
            Console.WriteLine($"  ✗ Error: {ex.Message}");
        }
    }

    static async Task TestPrintJob(IppServer server, TestSpoolerAdapter spooler)
    {
        var document = System.Text.Encoding.UTF8.GetBytes("Hello, World! This is a test document.");
        var request = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", document);
        Console.WriteLine($"  Request: {request.Length} bytes");

        using var input = new MemoryStream(request);
        using var output = new MemoryStream();

        try
        {
            await server.ProcessRequestAsync(input, output);

            var response = output.ToArray();
            Console.WriteLine($"  Response: {response.Length} bytes");

            if (spooler.PrintedJobs.Count > 0)
            {
                var job = spooler.PrintedJobs.Last();
                Console.WriteLine($"  ✓ Job created: ID={job.JobId}, Printer={job.PrinterName}");
                Console.WriteLine($"  ✓ Document: {job.Data.Length} bytes");
            }
            else
            {
                Console.WriteLine("  ✗ No jobs created (request may have failed)");
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine($"  ✗ Error: {ex.Message}");
        }
    }

    static async Task TestGetJobs(IppServer server)
    {
        var request = IppRequestBuilder.BuildGetJobsRequest();
        using var input = new MemoryStream(request);
        using var output = new MemoryStream();

        await server.ProcessRequestAsync(input, output);

        var response = output.ToArray();
        Console.WriteLine($"  Response: {response.Length} bytes");
        Console.WriteLine("  ✓ Get-Jobs completed");
    }

    static async Task TestCancelJob(IppServer server, TestSpoolerAdapter spooler)
    {
        // First create a job
        var document = System.Text.Encoding.UTF8.GetBytes("Test document for cancel");
        var printRequest = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", document);
        using var printInput = new MemoryStream(printRequest);
        using var printOutput = new MemoryStream();
        await server.ProcessRequestAsync(printInput, printOutput);

        if (spooler.PrintedJobs.Count == 0)
        {
            Console.WriteLine("  ✗ No jobs to cancel (Print-Job failed)");
            return;
        }

        var jobId = spooler.PrintedJobs.Last().JobId;
        Console.WriteLine($"  Created job {jobId} for cancellation test");

        // Cancel the job
        var cancelRequest = IppRequestBuilder.BuildCancelJobRequest(jobId);
        using var input = new MemoryStream(cancelRequest);
        using var output = new MemoryStream();

        await server.ProcessRequestAsync(input, output);

        var response = output.ToArray();
        Console.WriteLine($"  Response: {response.Length} bytes");
        Console.WriteLine("  ✓ Cancel-Job completed");
    }

    static async Task TestValidateJob(IppServer server)
    {
        var request = IppRequestBuilder.BuildValidateJobRequest("TestPrinter");
        using var input = new MemoryStream(request);
        using var output = new MemoryStream();

        await server.ProcessRequestAsync(input, output);

        var response = output.ToArray();
        Console.WriteLine($"  Response: {response.Length} bytes");
        Console.WriteLine("  ✓ Validate-Job completed");
    }
}

/// <summary>
/// Test spooler adapter for console app.
/// </summary>
public class TestSpoolerAdapter : ISpoolerAdapter
{
    private readonly List<PrinterInfo> _printers = new();
    private readonly List<PrintedJob> _printedJobs = new();
    private int _nextJobId = 1;

    public IReadOnlyList<PrintedJob> PrintedJobs => _printedJobs;

    public void AddPrinter(PrinterInfo printer) => _printers.Add(printer);

    public Task<SpoolerResult> PrintAsync(string printerName, byte[] data, string documentName, CancellationToken ct)
    {
        var printer = _printers.FirstOrDefault(p => p.Name == printerName);
        if (printer == null)
            return Task.FromResult(SpoolerResult.Fail($"Printer '{printerName}' not found"));

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
        => Task.FromResult<IReadOnlyList<PrinterInfo>>(_printers);
}

public class PrintedJob
{
    public int JobId { get; init; }
    public string PrinterName { get; init; } = string.Empty;
    public string DocumentName { get; init; } = string.Empty;
    public byte[] Data { get; init; } = Array.Empty<byte>();
}


