using System.Reflection;
using ShaPrint.Client;

namespace ShaPrint.Tests;

public class DriverRegistryEnumerationTests
{
    [Fact]
    public void RegistryFallback_ReadsDriverNamesBelowVersionKeys()
    {
        var registryTree = new Dictionary<string, IEnumerable<string>>(StringComparer.OrdinalIgnoreCase)
        {
            [string.Empty] = new[] { "Windows x64", "Windows NT x86" },
            [@"Windows x64\Drivers"] = new[] { "Version-3", "Version-4" },
            [@"Windows x64\Drivers\Version-3"] = new[] { "Canon LBP6030/6040/6018L" },
            [@"Windows x64\Drivers\Version-4"] = new[] { "Microsoft IPP Class Driver" },
            [@"Windows NT x86\Drivers"] = new[] { "Version-3" },
            [@"Windows NT x86\Drivers\Version-3"] = Array.Empty<string>(),
        };

        IEnumerable<string> ReadSubKeyNames(string path) =>
            registryTree.TryGetValue(path, out var names) ? names : Array.Empty<string>();

        MethodInfo? method = typeof(VirtualPrinterManager).GetMethod(
            "ReadRegisteredDriverNamesFromRegistry",
            BindingFlags.NonPublic | BindingFlags.Static);

        Assert.NotNull(method);

        var driverNames = Assert.IsType<List<string>>(method!.Invoke(
            null,
            new object?[] { (Func<string, IEnumerable<string>>)ReadSubKeyNames }));

        Assert.Contains("Canon LBP6030/6040/6018L", driverNames);
        Assert.Contains("Microsoft IPP Class Driver", driverNames);
        Assert.DoesNotContain("Version-3", driverNames);
        Assert.DoesNotContain("Version-4", driverNames);
    }
}
