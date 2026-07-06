using System;
using System.Collections.Generic;
using System.Linq;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.ViewModels.Pages;
using Xunit;

namespace ShaPrint.Tests
{
    public class MonitorViewModelTests
    {
        [Fact]
        public void UpdateServerStatus_DHCPChange_UpdatesExistingNodeIP()
        {
            // Arrange
            var vm = new MonitorViewModel();
            
            // Register a server initially with IP "192.168.1.10"
            vm.RegisterDiscoveredServers(new List<DiscoveryResponseMessage>
            {
                new DiscoveryResponseMessage { ServerName = "SRV-ROOM1", IpAddress = "192.168.1.10" }
            });

            Assert.Single(vm.Servers);
            Assert.Equal("192.168.1.10", vm.Servers[0].IpAddress);

            // Act: Receive update with same hostname but IP "192.168.1.25" (DHCP renewal)
            var payload = new ServerStatusPayload
            {
                HostName = "SRV-ROOM1",
                ServerName = "SRV-ROOM1",
                NetworkChannel = "DefaultChannel",
                Version = "1.0.0.0",
                UptimeSeconds = 120
            };
            vm.UpdateServerStatus(payload, "192.168.1.25", isOnline: true);

            // Assert: Still a single node, but IP is updated
            Assert.Single(vm.Servers);
            Assert.Equal("192.168.1.25", vm.Servers[0].IpAddress);
            Assert.Equal("Online", vm.Servers[0].Status);
        }

        [Fact]
        public void Filtering_FilterTextMatchesHostnameOrIP_CorrectlyFilters()
        {
            // Arrange
            var vm = new MonitorViewModel();
            vm.Servers.Add(new ServerNode { HostName = "SRV-ALPHA", IpAddress = "10.0.0.5", Status = "Online" });
            vm.Servers.Add(new ServerNode { HostName = "SRV-BETA", IpAddress = "192.168.1.50", Status = "Offline" });

            // Act & Assert: Empty filter shows all
            vm.FilterText = "";
            var items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Equal(2, items.Count);

            // Act & Assert: Filter by hostname substring
            vm.FilterText = "alpha";
            items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Single(items);
            Assert.Equal("SRV-ALPHA", items[0].HostName);

            // Act & Assert: Filter by IP substring
            vm.FilterText = ".1.50";
            items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Single(items);
            Assert.Equal("SRV-BETA", items[0].HostName);
        }

        [Fact]
        public void Sorting_OfflineFirst_PutsOfflineServersAtTop()
        {
            // Arrange
            var vm = new MonitorViewModel();
            vm.Servers.Add(new ServerNode { HostName = "SRV-A", Status = "Online" });
            vm.Servers.Add(new ServerNode { HostName = "SRV-B", Status = "Offline" });
            vm.Servers.Add(new ServerNode { HostName = "SRV-C", Status = "Warning" });

            // Act
            var sorted = vm.FilteredServers.Cast<ServerNode>().ToList();

            // Assert: Sort order should be Offline (1) -> Warning (2) -> Online (3)
            Assert.Equal(3, sorted.Count);
            Assert.Equal("SRV-B", sorted[0].HostName); // Offline
            Assert.Equal("SRV-C", sorted[1].HostName); // Warning
            Assert.Equal("SRV-A", sorted[2].HostName); // Online
        }
    }
}
