using System.Reflection;
using System.Text;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;
using Xunit;

namespace ShaPrint.Tests.Contract;

/// <summary>
/// Contract tests for <see cref="IPrintRelayClient"/> + the <see cref="PrintJobPayload"/>
/// wire format it carries.
///
/// Scope note (deliberate, see Task 10 report):
///  * The payload boundary limits (printer name too long → throw, spool data too large →
///    throw) are ALREADY covered by ShaPrint.Tests.PrintJobPayloadTests — this file does not
///    re-test them, it locks the complementary angle: the interface SIGNATURE and the
///    interface+payload round-trip at the valid boundary.
///  * "SendAsync honors hostOverride (no discovery)" is a behavior of the concrete clients,
///    not of the interface, and would need a real implementation + network or a mocking
///    framework (this project deliberately has none, to stay os-agnostic). The part of the
///    contract that IS enforceable here — hostOverride exists as an optional, defaulted
///    string? parameter — is asserted below.
/// </summary>
public class IPrintRelayClientContractTests
{
    private static readonly MethodInfo SendAsyncMethod =
        typeof(IPrintRelayClient).GetMethod(nameof(IPrintRelayClient.SendAsync))!;

    [Fact]
    public void SendAsync_HasStableParameterContract()
    {
        Assert.NotNull(SendAsyncMethod);

        var parameters = SendAsyncMethod.GetParameters();
        Assert.Equal(5, parameters.Length);

        Assert.Equal("targetPrinter", parameters[0].Name);
        Assert.Equal(typeof(string), parameters[0].ParameterType);

        Assert.Equal("data", parameters[1].Name);
        Assert.Equal(typeof(byte[]), parameters[1].ParameterType);

        Assert.Equal("documentName", parameters[2].Name);
        Assert.Equal(typeof(string), parameters[2].ParameterType);

        // hostOverride: nullable string, optional (default null) — lets a caller
        // bypass discovery and pin the relay target directly.
        Assert.Equal("hostOverride", parameters[3].Name);
        Assert.Equal(typeof(string), parameters[3].ParameterType);
        Assert.True(parameters[3].IsOptional);
        Assert.True(parameters[3].HasDefaultValue);
        Assert.Null(parameters[3].DefaultValue);

        // ct: CancellationToken, optional (default) — the wire client must stay
        // cooperative with cancellation.
        Assert.Equal("ct", parameters[4].Name);
        Assert.Equal(typeof(CancellationToken), parameters[4].ParameterType);
        Assert.True(parameters[4].IsOptional);

        Assert.Equal(typeof(Task<bool>), SendAsyncMethod.ReturnType);
    }

    [Fact]
    public async Task Payload_RoundTripsAtValidBoundaryPreservingContractFields()
    {
        // The payload type IPrintRelayClient.SendAsync accepts must survive the wire
        // format byte-for-byte. Uses the MAX-valid printer name (the over-limit case is
        // covered in ShaPrint.Tests.PrintJobPayloadTests) and a non-ASCII document name
        // to prove multi-byte UTF-8 round-trips.
        var payload = new PrintJobPayload
        {
            TargetPrinterName = new string('P', Constants.MaxTargetPrinterNameBytes),
            DocumentName = "Unicode-документ-📄.pdf",
            SpoolData = Encoding.UTF8.GetBytes("spool data for the relay client")
        };

        using var stream = new MemoryStream();
        await PrintJobPayload.WriteAsync(stream, payload);
        stream.Position = 0;

        var recovered = await PrintJobPayload.ReadAsync(stream);

        Assert.Equal(payload.TargetPrinterName, recovered.TargetPrinterName);
        Assert.Equal(payload.DocumentName, recovered.DocumentName);
        Assert.Equal(payload.SpoolData, recovered.SpoolData);
    }
}
