using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core.Network;
using ShaPrint.Client;
using ShaPrint.WpfApp.ViewModels.Pages;
using Xunit;

namespace ShaPrint.Tests.Services
{
    public class ServerReachabilityTrackerTests
    {
        // --- Test doubles ---

        private sealed class FakeListener
        {
            public string ServerIp { get; set; } = "";
            public bool Started { get; set; }
            public bool Stopped { get; set; }
        }

        private sealed class Harness
        {
            public List<InstalledPrinterConfig> Configs { get; } = new();
            public List<FakeListener> Listeners { get; } = new();
            public List<DiscoveryResponseMessage> NextScanResult { get; } = new();
            public int ScanCallCount { get; private set; }
            public List<ServerIdentityChangedArgs> ChangedEvents { get; } = new();
            public List<SuspiciousMatchArgs> SuspiciousEvents { get; } = new();

            public ServerReachabilityTracker BuildTracker()
            {
                return new ServerReachabilityTracker(
                    configProvider: () => Configs,
                    scanner: async () =>
                    {
                        ScanCallCount++;
                        await Task.Yield();
                        return NextScanResult.ToList();
                    },
                    onIdentityChanged: args =>
                    {
                        ChangedEvents.Add(args);
                        var oldFake = Listeners.FirstOrDefault(l => l.ServerIp == args.OldIp);
                        if (oldFake != null) oldFake.Stopped = true;
                        Listeners.Add(new FakeListener { ServerIp = args.NewIp, Started = true });
                    },
                    onSuspiciousMatch: args => SuspiciousEvents.Add(args),
                    debounceWindow: TimeSpan.FromMilliseconds(50)
                );
            }
        }

        // --- Tests ---

        [Fact]
        public async Task RequestRescan_UpdatesIp_WhenServerIdMatchesAndIpDiffers()
        {
            var h = new Harness();
            var cfg = new InstalledPrinterConfig
            {
                VirtualPrinterName = "VP1",
                PipeName = "pipe-A",
                ServerIp = "1.1.1.1",
                TargetPrinterName = "Printer1",
                DriverName = "Generic / Text Only",
                ServerId = "S1"
            };
            h.Configs.Add(cfg);
            var oldListener = new FakeListener { ServerIp = "1.1.1.1", Started = true };
            h.Listeners.Add(oldListener);

            h.NextScanResult.Add(new DiscoveryResponseMessage
            {
                ServerName = "ServerPC",
                IpAddress = "2.2.2.2",
                ServerId = "S1",
                ExposedPrinters = new List<PrinterInfo>
                {
                    new() { Name = "Printer1" }
                }
            });

            var tracker = h.BuildTracker();
            await tracker.RequestRescanAsync(ServerReachabilityTracker.RescanReason.Startup, CancellationToken.None);

            Assert.Equal(1, h.ScanCallCount);
            Assert.Equal("2.2.2.2", cfg.ServerIp);
            Assert.Single(h.ChangedEvents);
            Assert.Equal("S1", h.ChangedEvents[0].ServerId);
            Assert.Equal("1.1.1.1", h.ChangedEvents[0].OldIp);
            Assert.Equal("2.2.2.2", h.ChangedEvents[0].NewIp);
            Assert.True(oldListener.Stopped);
            Assert.Single(h.Listeners.Where(l => l.Started && !l.Stopped));
        }

        [Fact]
        public async Task RequestRescan_FallsBackToName_WhenServerIdMissingInConfig()
        {
            var h = new Harness();
            var cfg = new InstalledPrinterConfig
            {
                VirtualPrinterName = "ShaPrint [HomePC] - Printer1",
                PipeName = "pipe-B",
                ServerIp = "1.1.1.1",
                TargetPrinterName = "Printer1",
                DriverName = "Generic / Text Only",
                ServerId = null
            };
            h.Configs.Add(cfg);

            h.NextScanResult.Add(new DiscoveryResponseMessage
            {
                ServerName = "HomePC",
                IpAddress = "9.9.9.9",
                ExposedPrinters = new List<PrinterInfo> { new() { Name = "Printer1" } }
            });

            var tracker = h.BuildTracker();
            await tracker.RequestRescanAsync(ServerReachabilityTracker.RescanReason.ClientPageOpen, CancellationToken.None);

            Assert.Equal("9.9.9.9", cfg.ServerIp);
            Assert.Single(h.ChangedEvents);
        }

        [Fact]
        public async Task RequestRescan_EmitsSuspicious_WhenServerIdDiffers()
        {
            var h = new Harness();
            var cfg = new InstalledPrinterConfig
            {
                VirtualPrinterName = "ShaPrint [HomePC] - Printer1",
                PipeName = "pipe-C",
                ServerIp = "1.1.1.1",
                TargetPrinterName = "Printer1",
                DriverName = "Generic / Text Only",
                ServerId = "OLD-ID"
            };
            h.Configs.Add(cfg);

            h.NextScanResult.Add(new DiscoveryResponseMessage
            {
                ServerName = "HomePC",
                IpAddress = "1.1.1.1", // same IP, different ServerId
                ServerId = "NEW-ID",
                ExposedPrinters = new List<PrinterInfo> { new() { Name = "Printer1" } }
            });

            var tracker = h.BuildTracker();
            await tracker.RequestRescanAsync(ServerReachabilityTracker.RescanReason.Startup, CancellationToken.None);

            Assert.Empty(h.ChangedEvents);
            Assert.Single(h.SuspiciousEvents);
            Assert.Equal("1.1.1.1", cfg.ServerIp); // unchanged
        }

        [Fact]
        public async Task RequestRescan_NoMatch_DoesNotChange()
        {
            var h = new Harness();
            var cfg = new InstalledPrinterConfig
            {
                VirtualPrinterName = "ShaPrint [HomePC] - Printer1",
                PipeName = "pipe-D",
                ServerIp = "1.1.1.1",
                TargetPrinterName = "Printer1",
                DriverName = "Generic / Text Only",
                ServerId = "S1"
            };
            h.Configs.Add(cfg);

            h.NextScanResult.Add(new DiscoveryResponseMessage
            {
                ServerName = "OtherServer",
                IpAddress = "5.5.5.5",
                ServerId = "S9",
                ExposedPrinters = new List<PrinterInfo>()
            });

            var tracker = h.BuildTracker();
            await tracker.RequestRescanAsync(ServerReachabilityTracker.RescanReason.Startup, CancellationToken.None);

            Assert.Equal("1.1.1.1", cfg.ServerIp);
            Assert.Empty(h.ChangedEvents);
            Assert.Empty(h.SuspiciousEvents);
        }
    }
}
