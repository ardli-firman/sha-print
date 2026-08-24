# Gate 1 Build Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore a deterministic green Release build by correcting IPP framing, invalid-version responses, concurrent job state, concurrent printer routing, and the CI verification entry point.

**Architecture:** Keep the IPP request/response seam and existing SharpIppNext dependency. Correct malformed test request bytes at their source, keep protocol error mapping inside `IppServer`, replace mutable shared dictionaries/counters with concurrent primitives, and make one PowerShell verification script the command used locally and by both release workflows.

**Tech Stack:** .NET 8, C#, SharpIppNext 4.2.1, xUnit 2.5.3, PowerShell 7, GitHub Actions Windows runners.

## Global Constraints

- Target one server and approximately three clients per room.
- Support Windows 10 and Windows 11 x64.
- Keep IPP/driverless and all legacy features enabled.
- Permit trusted-LAN IPP without additional authentication.
- Do not change Network Channel defaults or security in this gate.
- Use test-first changes and make one behavior change per commit.
- Do not suppress existing compiler warnings as part of this gate.

## File Structure

- `ShaPrint.Core/Ipp/Testing/IppRequestBuilder.cs` — construct standards-shaped raw IPP request fixtures.
- `ShaPrint.Core/Ipp/IppServer.cs` — map IPP requests to responses and own thread-safe in-memory job state.
- `ShaPrint.Core/Ipp/IppHttpServer.cs` — route concurrent HTTP requests to one stable server instance per printer.
- `ShaPrint.Tests/IppRequestBuilderTests.cs` — direct framing regression tests for request fixtures.
- `ShaPrint.Tests/IppServerTests.cs` — request/response and concurrent job regression tests; test spooler becomes thread-safe.
- `ShaPrint.Tests/IppHttpServerTests.cs` — concurrent printer-route cache regression test.
- `scripts/verify-build.ps1` — deterministic restore/build/test command used locally and in CI.
- `.github/workflows/beta-release.yml` — invoke the shared verification command before publishing beta.
- `.github/workflows/stable-release.yml` — invoke the shared verification command before publishing stable.

---

### Task 1: Correct IPP Print-Job Fixture Framing

**Files:**
- Create: `ShaPrint.Tests/IppRequestBuilderTests.cs`
- Modify: `ShaPrint.Core/Ipp/Testing/IppRequestBuilder.cs`
- Test: `ShaPrint.Tests/IppRequestBuilderTests.cs`
- Test: `ShaPrint.Tests/IppServerTests.cs`

**Interfaces:**
- Consumes: `IppRequestBuilder.BuildPrintJobRequest(string printerName, byte[] documentData, string? documentFormat)`.
- Produces: A request whose single `0x03` end-of-attributes delimiter is immediately followed by exactly the caller-provided document bytes.

- [ ] **Step 1: Write the direct framing regression test**

Create `ShaPrint.Tests/IppRequestBuilderTests.cs`:

```csharp
using ShaPrint.Core.Ipp.Testing;

namespace ShaPrint.Tests;

public class IppRequestBuilderTests
{
    [Fact]
    public void BuildPrintJobRequest_EndsWithExactDocumentBytes()
    {
        byte[] document = [0x50, 0x44, 0x46, 0x7F];

        byte[] request = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", document);

        Assert.True(request.AsSpan().EndsWith(document));
        Assert.Equal(document.Length, request.AsSpan(request.Length - document.Length).Length);
    }
}
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```powershell
dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj -c Release --no-restore --filter "FullyQualifiedName~IppRequestBuilderTests.BuildPrintJobRequest_EndsWithExactDocumentBytes|FullyQualifiedName~IppServerTests.PrintJob_SendsDataToSpooler_ReturnsJobId|FullyQualifiedName~IppServerTests.PrintJob_LargeDocument_Works"
```

Expected: all three tests fail because `BuildPrintJobRequest` appends an extra `0x03` after the document.

- [ ] **Step 3: Remove the trailing byte after the document**

In `IppRequestBuilder.BuildPrintJobRequest`, leave the attribute delimiter in place and write only the document afterwards:

```csharp
writer.Write((byte)0x03); // end-of-attributes; document begins immediately after this byte

writer.Write(documentData);

return ms.ToArray();
```

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run the Step 2 command again.

Expected: 3 passed, 0 failed; captured spool data is byte-for-byte identical to the original payload.

- [ ] **Step 5: Commit the framing fix**

```powershell
git add ShaPrint.Core/Ipp/Testing/IppRequestBuilder.cs ShaPrint.Tests/IppRequestBuilderTests.cs
git commit -m "fix(ipp): preserve exact print document bytes"
```

---

### Task 2: Return a Valid Response for Unsupported IPP Versions

**Files:**
- Modify: `ShaPrint.Core/Ipp/IppServer.cs`
- Modify: `ShaPrint.Tests/IppServerTests.cs`

**Interfaces:**
- Consumes: `IppRequestException.RequestMessage`, including invalid request version and request ID.
- Produces: An IPP 1.1 response with the original request ID and `ServerErrorVersionNotSupported` status when SharpIpp rejects the request version.

- [ ] **Step 1: Strengthen the existing invalid-version test**

Replace the empty assertion section of `InvalidIppVersion_ReturnsError` with:

```csharp
var responseBytes = outputStream.ToArray();
Assert.True(responseBytes.Length >= 8);
Assert.Equal(0x01, responseBytes[0]);
Assert.Equal(0x01, responseBytes[1]);
Assert.Equal(0x05, responseBytes[2]);
Assert.Equal(0x03, responseBytes[3]);
Assert.Equal(1, System.Buffers.Binary.BinaryPrimitives.ReadInt32BigEndian(responseBytes.AsSpan(4, 4)));
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```powershell
dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj -c Release --no-restore --filter "FullyQualifiedName~IppServerTests.InvalidIppVersion_ReturnsError"
```

Expected: FAIL with `SharpIpp.Exceptions.IppResponseException: Unsupported IPP version` from `SendRawResponseAsync`.

- [ ] **Step 3: Normalize only unsupported versions**

In the `catch (IppRequestException ex)` block of `IppServer.ProcessRequestAsync`, construct the response with a supported version only for the version error:

```csharp
var responseVersion = ex.StatusCode == IppStatusCode.ServerErrorVersionNotSupported
    ? new IppVersion(1, 1)
    : ex.RequestMessage.Version;

var errorResponse = new IppResponseMessage
{
    RequestId = ex.RequestMessage.RequestId,
    Version = responseVersion,
    StatusCode = ex.StatusCode
};
```

Keep the existing required charset and natural-language operation attributes.

- [ ] **Step 4: Run the focused test and verify GREEN**

Run the Step 2 command again.

Expected: 1 passed, 0 failed; response header is IPP 1.1 / `0x0503` / request ID 1.

- [ ] **Step 5: Commit the response fix**

```powershell
git add ShaPrint.Core/Ipp/IppServer.cs ShaPrint.Tests/IppServerTests.cs
git commit -m "fix(ipp): normalize unsupported version responses"
```

---

### Task 3: Make IPP Job State Safe Under Concurrent Requests

**Files:**
- Modify: `ShaPrint.Core/Ipp/IppServer.cs`
- Modify: `ShaPrint.Tests/IppServerTests.cs`

**Interfaces:**
- Consumes: concurrent calls to `JobManager.CreateJob(string printerName, string documentName)`.
- Produces: unique positive integer IDs via `Interlocked.Increment`, retained jobs via `ConcurrentDictionary<int, IppJob>`, snapshot reads from `GetAllJobs()`, and an independent `SharpIppServer` protocol object per request.

- [ ] **Step 1: Add a deterministic concurrent JobManager test**

Add inside `IppServerTests`:

```csharp
[Fact]
public void JobManager_CreateJobConcurrently_RetainsUniqueJobs()
{
    var manager = new JobManager();

    Parallel.For(0, 1_000, i => manager.CreateJob("TestPrinter", $"Job-{i}"));

    var jobs = manager.GetAllJobs();
    Assert.Equal(1_000, jobs.Count);
    Assert.Equal(1_000, jobs.Select(job => job.Id).Distinct().Count());
    Assert.DoesNotContain(jobs, job => job.Id <= 0);
}
```

- [ ] **Step 2: Make the test spooler safe so the server test measures server behavior**

Change the test-only `InMemorySpoolerAdapter` fields and snapshot property:

```csharp
private readonly List<PrinterInfo> _printers = new();
private readonly System.Collections.Concurrent.ConcurrentQueue<PrintedJob> _printedJobs = new();
private int _nextJobId;

public IReadOnlyList<PrintedJob> PrintedJobs => _printedJobs.ToArray();
```

Change its `PrintAsync` ID and append operations:

```csharp
var jobId = Interlocked.Increment(ref _nextJobId);
_printedJobs.Enqueue(new PrintedJob
{
    JobId = jobId,
    PrinterName = job.PrinterName,
    DocumentName = job.DocumentName,
    Data = job.Data,
    DocumentFormat = job.DocumentFormat
});
```

- [ ] **Step 3: Strengthen the existing server concurrency assertion**

In `ConcurrentRequests_DoNotCrash`, send 100 requests instead of 10 and assert both count and unique test-spooler IDs:

```csharp
const int requestCount = 100;
var tasks = Enumerable.Range(0, requestCount).Select(async _ =>
{
    var request = IppRequestBuilder.BuildPrintJobRequest(
        "TestPrinter", [0x50, 0x44, 0x46]);
    using var input = new MemoryStream(request);
    using var output = new MemoryStream();
    await server.ProcessRequestAsync(input, output);
}).ToArray();

await Task.WhenAll(tasks);

Assert.Equal(requestCount, spooler.PrintedJobs.Count);
Assert.Equal(requestCount, spooler.PrintedJobs.Select(job => job.JobId).Distinct().Count());
```

- [ ] **Step 4: Run concurrent tests and verify RED**

Run:

```powershell
dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj -c Release --no-restore --filter "FullyQualifiedName~IppServerTests.JobManager_CreateJobConcurrently_RetainsUniqueJobs|FullyQualifiedName~IppServerTests.ConcurrentRequests_DoNotCrash"
```

Expected: `JobManager_CreateJobConcurrently_RetainsUniqueJobs` fails through lost/duplicate jobs or concurrent dictionary mutation. The server concurrency test must complete without hidden exceptions; if it independently exposes SharpIpp instance state, retain that failure as evidence for a per-request protocol instance.

- [ ] **Step 5: Replace shared protocol and JobManager state with request-local/concurrent primitives**

Remove the `_ippProtocol` field and both constructor assignments. At the start of `ProcessRequestAsync`, create a protocol object local to this request and use it for all parse/serialize/send calls:

```csharp
var ippProtocol = new SharpIppServer();
IIppRequest request = await ippProtocol.ReceiveRequestAsync(inputStream);
// ... dispatch and build response ...
IIppResponseMessage rawResponse = await ippProtocol.CreateRawResponseAsync(response);
await ippProtocol.SendRawResponseAsync(rawResponse, outputStream);
```

Use the same local `ippProtocol` in the `IppRequestException` catch block when sending the error response. Then update the `JobManager` fields and methods in `IppServer.cs`:

```csharp
private readonly System.Collections.Concurrent.ConcurrentDictionary<int, IppJob> _jobs = new();
private int _nextJobId;

public int ActiveJobCount => _jobs.Values.Count(job =>
    job.State == JobState.Pending || job.State == JobState.Processing);

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
        throw new InvalidOperationException($"IPP job ID collision: {id}.");

    return job;
}

public IppJob? GetJob(int jobId) =>
    _jobs.TryGetValue(jobId, out var job) ? job : null;

public IReadOnlyList<IppJob> GetAllJobs() =>
    _jobs.Values.OrderBy(job => job.Id).ToArray();
```

Leave `CancelJob` using `TryGetValue`; state-transition locking is deferred unless a failing transition race test demonstrates it in Gate 2.

- [ ] **Step 6: Run concurrent tests repeatedly and verify GREEN**

Run:

```powershell
1..10 | ForEach-Object {
    dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj -c Release --no-build --filter "FullyQualifiedName~IppServerTests.JobManager_CreateJobConcurrently_RetainsUniqueJobs|FullyQualifiedName~IppServerTests.ConcurrentRequests_DoNotCrash"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
```

Expected: all 10 runs pass with no parser state shared between concurrent HTTP requests.

- [ ] **Step 7: Commit the concurrent job fix**

```powershell
git add ShaPrint.Core/Ipp/IppServer.cs ShaPrint.Tests/IppServerTests.cs
git commit -m "fix(ipp): make job state concurrency safe"
```

---

### Task 4: Make Per-Printer HTTP Routing Atomic

**Files:**
- Modify: `ShaPrint.Core/Ipp/IppHttpServer.cs`
- Create: `ShaPrint.Tests/IppHttpServerTests.cs`

**Interfaces:**
- Consumes: concurrent calls to `IppHttpServer.GetOrCreatePrinterServer(string printerName)`.
- Produces: one stable `IppServer` instance per case-insensitive printer name via `ConcurrentDictionary<string, Lazy<IppServer>>`.

- [ ] **Step 1: Expose the existing cache seam to tests and add the failing test**

Change only the method visibility from `private` to `internal`, then create `ShaPrint.Tests/IppHttpServerTests.cs`:

```csharp
using System.Collections.Concurrent;
using ShaPrint.Core.Ipp;

namespace ShaPrint.Tests;

public class IppHttpServerTests
{
    [Fact]
    public void GetOrCreatePrinterServer_ConcurrentSameName_ReturnsOneInstance()
    {
        var spooler = new InMemorySpoolerAdapter();
        var httpServer = new IppHttpServer(spooler, port: 16310);
        var instances = new ConcurrentBag<IppServer>();

        Parallel.For(0, 1_000, _ =>
            instances.Add(httpServer.GetOrCreatePrinterServer("Office Printer")));

        Assert.Single(instances.Distinct());
    }
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```powershell
dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj -c Release --no-restore --filter "FullyQualifiedName~IppHttpServerTests.GetOrCreatePrinterServer_ConcurrentSameName_ReturnsOneInstance"
```

Expected: FAIL because the check-then-add `Dictionary` path can return multiple instances for the same printer, or throw during concurrent mutation.

- [ ] **Step 3: Make cache creation atomic and case-insensitive**

Replace the cache field and method:

```csharp
private readonly System.Collections.Concurrent.ConcurrentDictionary<string, Lazy<IppServer>> _printerServers =
    new(StringComparer.OrdinalIgnoreCase);

internal IppServer GetOrCreatePrinterServer(string printerName)
{
    var lazyServer = _printerServers.GetOrAdd(
        printerName,
        name => new Lazy<IppServer>(
            () =>
            {
                AppLogger.Log($"[IPP] Created server instance for printer: {name}");
                return new IppServer(_spooler, name);
            },
            LazyThreadSafetyMode.ExecutionAndPublication));

    return lazyServer.Value;
}
```

- [ ] **Step 4: Run the routing and existing printer-path tests**

Run:

```powershell
dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj -c Release --no-restore --filter "FullyQualifiedName~IppHttpServerTests|FullyQualifiedName~IppPrinterTests"
```

Expected: all matching tests pass and all concurrent callers receive the same instance.

- [ ] **Step 5: Commit the routing fix**

```powershell
git add ShaPrint.Core/Ipp/IppHttpServer.cs ShaPrint.Tests/IppHttpServerTests.cs
git commit -m "fix(ipp): make printer routing atomic"
```

---

### Task 5: Add One Deterministic Build Verification Command

**Files:**
- Create: `scripts/verify-build.ps1`
- Modify: `.github/workflows/beta-release.yml`
- Modify: `.github/workflows/stable-release.yml`

**Interfaces:**
- Consumes: repository root containing `ShaPrint.sln` and .NET 8 SDK.
- Produces: exit code 0 only after restore, Release build, and Release tests succeed; writes TRX results to `TestResults`.

- [ ] **Step 1: Create the verification script**

Create `scripts/verify-build.ps1`:

```powershell
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$solution = Join-Path $repoRoot 'ShaPrint.sln'
$results = Join-Path $repoRoot 'TestResults'

Push-Location $repoRoot
try {
    dotnet restore $solution --disable-parallel
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    dotnet build $solution -c Release --no-restore -m:1 -p:UseSharedCompilation=false
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    dotnet test 'ShaPrint.Tests/ShaPrint.Tests.csproj' -c Release --no-restore --no-build `
        --logger 'trx;LogFileName=ShaPrint.Tests.trx' `
        --results-directory $results
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
}
```

- [ ] **Step 2: Run the script locally**

Run:

```powershell
pwsh -NoProfile -File scripts/verify-build.ps1
```

Expected: exit code 0, Release build succeeds, 0 failed tests, and `TestResults/ShaPrint.Tests.trx` exists. Existing compiler warnings remain visible and are not promoted to failures in Gate 1.

- [ ] **Step 3: Replace duplicated workflow test commands**

In both `.github/workflows/beta-release.yml` and `.github/workflows/stable-release.yml`, replace:

```yaml
- name: Run Tests
  run: dotnet test -c Release
```

with:

```yaml
- name: Verify Release Build
  shell: pwsh
  run: ./scripts/verify-build.ps1

- name: Upload Test Results
  if: always()
  uses: actions/upload-artifact@v4
  with:
    name: test-results-${{ github.job }}-${{ github.run_attempt }}
    path: TestResults/*.trx
    if-no-files-found: error
```

Add `'scripts/**'` to each workflow's positive `paths` list so changes to the verification entry point trigger release verification.

- [ ] **Step 4: Validate workflow text and rerun local verification**

Run:

```powershell
rg -n "Verify Release Build|verify-build.ps1|Upload Test Results|scripts/\*\*" .github/workflows/beta-release.yml .github/workflows/stable-release.yml
pwsh -NoProfile -File scripts/verify-build.ps1
```

Expected: each of the four workflow markers appears in both workflows; local verification exits 0.

- [ ] **Step 5: Commit the shared verification gate**

```powershell
git add scripts/verify-build.ps1 .github/workflows/beta-release.yml .github/workflows/stable-release.yml
git commit -m "ci: add deterministic release verification"
```

---

### Task 6: Gate 1 Final Verification and Evidence

**Files:**
- Create: `docs/production-readiness/gate-1-build-foundation.md`
- Verify: all Gate 1 modified files.

**Interfaces:**
- Consumes: Tasks 1-5.
- Produces: Gate 1 evidence with exact commands, result counts, known warnings, and remaining Gate 2 risks.

- [ ] **Step 1: Run the original four-failure reproduction**

Run:

```powershell
dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj -c Release --no-build --filter "FullyQualifiedName~IppServerTests.PrintJob_SendsDataToSpooler_ReturnsJobId|FullyQualifiedName~IppServerTests.PrintJob_LargeDocument_Works|FullyQualifiedName~IppServerTests.InvalidIppVersion_ReturnsError|FullyQualifiedName~IppServerTests.ConcurrentRequests_DoNotCrash"
```

Expected: 4 passed, 0 failed.

- [ ] **Step 2: Stress the concurrency regressions**

Run:

```powershell
1..20 | ForEach-Object {
    dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj -c Release --no-build --filter "FullyQualifiedName~JobManager_CreateJobConcurrently_RetainsUniqueJobs|FullyQualifiedName~ConcurrentRequests_DoNotCrash|FullyQualifiedName~GetOrCreatePrinterServer_ConcurrentSameName_ReturnsOneInstance"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
```

Expected: all 20 runs pass.

- [ ] **Step 3: Run the canonical verification command**

Run:

```powershell
pwsh -NoProfile -File scripts/verify-build.ps1
```

Expected: restore, Release build, and all tests pass; TRX exists.

- [ ] **Step 4: Confirm no debug residue and review the diff**

Run:

```powershell
rg -n "\[DEBUG-" ShaPrint.Core/Ipp ShaPrint.Tests/Ipp* scripts/verify-build.ps1
git diff --check
git status --short
git diff --stat origin/develop...HEAD
```

Expected: no `[DEBUG-` instrumentation, no newly introduced placeholder markers, no whitespace errors, and only intended Gate 1 files differ.

- [ ] **Step 5: Write Gate 1 evidence**

Create `docs/production-readiness/gate-1-build-foundation.md` with this exact structure after Steps 1-4 produce their expected passing results:

```markdown
# Gate 1: Build Foundation Evidence

## Result

PASS

## Verified Commit

The verified code is the parent of the commit that adds this evidence file.

## Commands

- Original IPP regression filter: 4/4 passed
- Concurrency stress loop: 20/20 runs passed
- Canonical verification: 391/391 tests passed

## Build Warnings

- Existing obsolete legacy-service and nullable-analysis warnings remain visible in the canonical verification log; Gate 1 adds no warning suppression.

## Gate 2 Inputs

- Version and acknowledge the legacy TCP protocol.
- Add bounded network and spooler timeouts.
- Normalize actionable failure categories.
- Test cleanup and overload behavior.
```

- [ ] **Step 6: Commit Gate 1 evidence**

```powershell
git add -f docs/production-readiness/gate-1-build-foundation.md
git commit -m "docs: record gate 1 verification evidence"
```
