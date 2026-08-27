using System.Reflection;
using ShaPrint.Platform.Abstractions;
using Xunit;

namespace ShaPrint.Tests.Contract;

/// <summary>
/// Contract tests for <see cref="IVirtualPrinterManager"/>.
///
/// These tests lock the platform abstraction surface so every implementation
/// (Windows driver manager, Unix CUPS backend, future Android backend) must
/// satisfy the same shape. They are os-agnostic by construction — they only
/// reflect over the interface — so CI runs them on windows, ubuntu AND macos.
///
/// In practice this is "compiler-enforced": a backend compiled against a
/// changed interface (e.g. one that re-introduces a `pipeName` parameter)
/// would fail to compile; these tests additionally assert the contract at
/// runtime so the drift is caught even for reflection-based implementers.
/// </summary>
public class IVirtualPrinterManagerContractTests
{
    private static readonly Type InterfaceType = typeof(IVirtualPrinterManager);

    // The whole point of the abstraction: the printer-name plumbing is an
    // implementation detail, NOT part of the interface. No method may accept
    // a parameter named pipeName/pipe.
    private static readonly string[] ForbiddenParameterNames = { "pipeName", "pipe" };

    [Fact]
    public void InstallPrinterAsync_HasExactlyTwoStringParameters()
    {
        var method = InterfaceType.GetMethod(nameof(IVirtualPrinterManager.InstallPrinterAsync));
        Assert.NotNull(method);

        var parameters = method.GetParameters();
        Assert.Equal(2, parameters.Length);

        Assert.Equal("virtualPrinterName", parameters[0].Name);
        Assert.Equal(typeof(string), parameters[0].ParameterType);

        Assert.Equal("driverName", parameters[1].Name);
        Assert.Equal(typeof(string), parameters[1].ParameterType);
    }

    [Fact]
    public void NoMethodAcceptsPipeNameOrPipeParameter()
    {
        foreach (var method in InterfaceType.GetMethods())
        {
            foreach (var parameter in method.GetParameters())
            {
                // "pipeName" would leak the Windows named-pipe implementation
                // into the abstraction — a backend can't be interchangeable then.
                Assert.DoesNotContain(
                    parameter.Name,
                    ForbiddenParameterNames,
                    StringComparer.OrdinalIgnoreCase);
            }
        }
    }

    [Fact]
    public void InstallPrinterAsync_ReturnsSuccessAndErrorMessageTuple()
    {
        var method = InterfaceType.GetMethod(nameof(IVirtualPrinterManager.InstallPrinterAsync));
        Assert.NotNull(method);

        // Task<(bool Success, string ErrorMessage)> — tuple element names are not
        // part of the CLR type, so typeof(Task<(bool, string)>) matches exactly.
        Assert.Equal(typeof(Task<(bool, string)>), method.ReturnType);
    }

    [Fact]
    public void RemovePrinterAsync_HasExactlyOneParameter()
    {
        var method = InterfaceType.GetMethod(nameof(IVirtualPrinterManager.RemovePrinterAsync));
        Assert.NotNull(method);

        var parameters = method.GetParameters();
        Assert.Single(parameters);

        Assert.Equal("virtualPrinterName", parameters[0].Name);
        Assert.Equal(typeof(string), parameters[0].ParameterType);
    }

    [Fact]
    public void GetInstalledVirtualPrinters_ExistsAndReturnsStringList()
    {
        var method = InterfaceType.GetMethod(nameof(IVirtualPrinterManager.GetInstalledVirtualPrinters));
        Assert.NotNull(method);

        Assert.Empty(method.GetParameters());
        Assert.Equal(typeof(List<string>), method.ReturnType);
    }
}
