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

    [Fact]
    public void GetOrCreatePrinterServer_DifferentCasing_ReturnsSameInstance()
    {
        var spooler = new InMemorySpoolerAdapter();
        var httpServer = new IppHttpServer(spooler, port: 16310);

        var canonical = httpServer.GetOrCreatePrinterServer("Office Printer");
        var differentCasing = httpServer.GetOrCreatePrinterServer("office printer");

        Assert.Same(canonical, differentCasing);
    }
}
