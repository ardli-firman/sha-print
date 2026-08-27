using System.Reflection;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;
using Xunit;

namespace ShaPrint.Tests.Contract;

/// <summary>
/// Contract tests for <see cref="IPrinterManager"/> — the os-agnostic print
/// surface used by the UI. Locks the two public operations and their signatures
/// so Windows (Spooler) and Unix (CUPS) backends stay interchangeable.
/// </summary>
public class IPrinterManagerContractTests
{
    [Fact]
    public void GetLocalPrintersAsync_HasNoParametersAndReturnsPrinterList()
    {
        var method = typeof(IPrinterManager).GetMethod(nameof(IPrinterManager.GetLocalPrintersAsync));
        Assert.NotNull(method);

        Assert.Empty(method.GetParameters());
        // Ties the abstraction to Core's model: Task<List<PrinterInfo>>.
        Assert.Equal(typeof(Task<List<PrinterInfo>>), method.ReturnType);
    }

    [Fact]
    public void PrintRawDataAsync_HasOptionalTimeoutAsLastParameter()
    {
        var method = typeof(IPrinterManager).GetMethod(nameof(IPrinterManager.PrintRawDataAsync));
        Assert.NotNull(method);

        var parameters = method.GetParameters();
        Assert.Equal(4, parameters.Length);

        Assert.Equal("printerName", parameters[0].Name);
        Assert.Equal(typeof(string), parameters[0].ParameterType);

        Assert.Equal("data", parameters[1].Name);
        Assert.Equal(typeof(byte[]), parameters[1].ParameterType);

        Assert.Equal("documentName", parameters[2].Name);
        Assert.Equal(typeof(string), parameters[2].ParameterType);

        // timeout: nullable TimeSpan, optional — a caller may rely on the backend
        // default; the signature must not grow a required timeout.
        Assert.Equal("timeout", parameters[3].Name);
        Assert.Equal(typeof(TimeSpan?), parameters[3].ParameterType);
        Assert.True(parameters[3].IsOptional);

        Assert.Equal(typeof(Task<bool>), method.ReturnType);
    }
}
