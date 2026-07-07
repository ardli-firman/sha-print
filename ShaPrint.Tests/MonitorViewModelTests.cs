using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Linq;
using System.Windows.Media;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.ViewModels.Pages;
using Xunit;

namespace ShaPrint.Tests
{
    public class MonitorViewModelTests
    {
        // ── ServerNode Tests ──────────────────────────────────────────

        [Theory]
        [InlineData("Online", 3)]
        [InlineData("Warning", 2)]
        [InlineData("Offline", 1)]
        [InlineData("Unknown", 4)]
        [InlineData("SomeRandomState", 4)]
        public void ServerNode_StatusSortOrder_ReturnsCorrectOrder(string status, int expectedOrder)
        {
            var node = new ServerNode { Status = status };
            Assert.Equal(expectedOrder, node.StatusSortOrder);
        }

        [Theory]
        [InlineData("Online")]
        [InlineData("Warning")]
        [InlineData("Offline")]
        [InlineData("Unknown")]
        public void ServerNode_Brushes_ReturnFallbackColorsInUnitTest(string status)
        {
            var node = new ServerNode { Status = status };

            // In unit tests, Application.Current is null, so it falls back to standard brushes
            var brush = node.StatusBrush as SolidColorBrush;
            Assert.NotNull(brush);

            Color expectedColor = status switch
            {
                "Online" => Colors.Green,
                "Warning" => Colors.Orange,
                "Offline" => Colors.Red,
                _ => Colors.Gray
            };

            Assert.Equal(expectedColor, brush.Color);
            Assert.Equal(brush, node.StatusTextBrush);

            var bgBrush = node.StatusBackgroundBrush as SolidColorBrush;
            Assert.NotNull(bgBrush);
            Assert.Equal(26, bgBrush.Color.A); // 26 opacity (~10%)
            Assert.Equal(expectedColor.R, bgBrush.Color.R);
            Assert.Equal(expectedColor.G, bgBrush.Color.G);
            Assert.Equal(expectedColor.B, bgBrush.Color.B);

            var borderBrush = node.StatusBorderBrush as SolidColorBrush;
            Assert.NotNull(borderBrush);
            Assert.Equal(76, borderBrush.Color.A); // 76 opacity (~30%)
            Assert.Equal(expectedColor.R, borderBrush.Color.R);
            Assert.Equal(expectedColor.G, borderBrush.Color.G);
            Assert.Equal(expectedColor.B, borderBrush.Color.B);
        }

        [Theory]
        [InlineData(-10, "0s")]
        [InlineData(0, "0s")]
        [InlineData(45, "0m 45s")]
        [InlineData(150, "2m 30s")]
        [InlineData(3750, "1h 2m 30s")]
        [InlineData(90000, "1d 1h 0m")]
        public void ServerNode_UptimeText_FormatsCorrectly(long seconds, string expectedText)
        {
            var node = new ServerNode { UptimeSeconds = seconds };
            Assert.Equal(expectedText, node.UptimeText);
        }

        [Fact]
        public void ServerNode_LastSeenText_FormatsCorrectly()
        {
            var now = DateTime.UtcNow;

            var node1 = new ServerNode { LastSeen = now.AddSeconds(-2) };
            Assert.Equal("Just now", node1.LastSeenText);

            var node2 = new ServerNode { LastSeen = now.AddSeconds(-45) };
            Assert.Equal("45s ago", node2.LastSeenText);

            var node3 = new ServerNode { LastSeen = now.AddMinutes(-15) };
            Assert.Equal("15m ago", node3.LastSeenText);

            var node4 = new ServerNode { LastSeen = now.AddHours(-3) };
            Assert.Equal("3h ago", node4.LastSeenText);
        }

        [Fact]
        public void ServerNode_VersionAndNetworkChannel_ReturnPayloadValuesOrUnknown()
        {
            var node = new ServerNode();
            Assert.Equal("Unknown", node.Version);
            Assert.Equal("Unknown", node.NetworkChannel);

            node.Payload = new ServerStatusPayload
            {
                Version = "2.1.0",
                NetworkChannel = "Production"
            };

            Assert.Equal("2.1.0", node.Version);
            Assert.Equal("Production", node.NetworkChannel);
        }

        // ── MonitorViewModel Property / Summary Tests ──────────────────

        [Fact]
        public void MonitorViewModel_LastRefreshText_FormatsCorrectly()
        {
            var vm = new MonitorViewModel();
            Assert.Equal("Never", vm.LastRefreshText);

            var now = DateTime.UtcNow;

            vm.LastRefreshTime = now.AddSeconds(-3);
            Assert.Equal("Just now", vm.LastRefreshText);

            vm.LastRefreshTime = now.AddSeconds(-20);
            Assert.Equal("20s ago", vm.LastRefreshText);

            vm.LastRefreshTime = now.AddMinutes(-5);
            Assert.Equal("5m ago", vm.LastRefreshText);

            vm.LastRefreshTime = now.AddHours(-2);
            Assert.Equal("2h ago", vm.LastRefreshText);
        }

        [Fact]
        public void MonitorViewModel_SummaryCounts_UpdateCorrectly()
        {
            var vm = new MonitorViewModel();
            
            // Helper to invoke private UpdateSummary via reflection
            void TriggerUpdateSummary()
            {
                var method = typeof(MonitorViewModel).GetMethod("UpdateSummary", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
                method!.Invoke(vm, null);
            }

            TriggerUpdateSummary();
            Assert.True(vm.IsEmpty);
            Assert.Equal(0, vm.TotalServers);

            // Add server nodes
            var s1 = new ServerNode { HostName = "S1", Status = "Online" };
            var s2 = new ServerNode { HostName = "S2", Status = "Warning" };
            var s3 = new ServerNode { HostName = "S3", Status = "Offline" };
            var s4 = new ServerNode { HostName = "S4", Status = "Unknown" };

            vm.Servers.Add(s1);
            vm.Servers.Add(s2);
            vm.Servers.Add(s3);
            vm.Servers.Add(s4);

            TriggerUpdateSummary();

            Assert.False(vm.IsEmpty);
            Assert.Equal(4, vm.TotalServers);
            Assert.Equal(1, vm.OnlineCount);
            Assert.Equal(1, vm.WarningCount);
            Assert.Equal(1, vm.OfflineCount);
            Assert.Equal(1, vm.UnknownCount);
            Assert.False(vm.HasAnyErrors);

            // Add payload errors to check HasAnyErrors
            s1.Payload = new ServerStatusPayload
            {
                Errors = new List<ServerErrorEntry> { new ServerErrorEntry { Message = "Err" } }
            };

            TriggerUpdateSummary();
            Assert.True(vm.HasAnyErrors);
        }

        // ── MonitorViewModel Commands Tests ────────────────────────────

        [Fact]
        public void MonitorViewModel_ToggleExpandCommand_TogglesExpansion()
        {
            var vm = new MonitorViewModel();
            var node = new ServerNode { IsExpanded = false };

            vm.ToggleExpandCommand.Execute(node);
            Assert.True(node.IsExpanded);

            vm.ToggleExpandCommand.Execute(node);
            Assert.False(node.IsExpanded);
        }

        [Fact]
        public void MonitorViewModel_ExpandAndCollapseAllCommands_Work()
        {
            var vm = new MonitorViewModel();
            var n1 = new ServerNode { IsExpanded = false };
            var n2 = new ServerNode { IsExpanded = false };
            vm.Servers.Add(n1);
            vm.Servers.Add(n2);

            vm.ExpandAllCommand.Execute(null);
            Assert.True(n1.IsExpanded);
            Assert.True(n2.IsExpanded);

            vm.CollapseAllCommand.Execute(null);
            Assert.False(n1.IsExpanded);
            Assert.False(n2.IsExpanded);
        }

        [Fact]
        public void MonitorViewModel_SetStatusFilterCommand_SetsFilter()
        {
            var vm = new MonitorViewModel();
            Assert.Equal("All", vm.ActiveStatusFilter);

            vm.SetStatusFilterCommand.Execute("Warning");
            Assert.Equal("Warning", vm.ActiveStatusFilter);
        }

        // ── MonitorViewModel Server List Management Tests ──────────────

        [Fact]
        public void UpdateServerStatus_DHCPChange_UpdatesExistingNodeIP()
        {
            var vm = new MonitorViewModel();
            
            vm.RegisterDiscoveredServers(new List<DiscoveryResponseMessage>
            {
                new DiscoveryResponseMessage { ServerName = "SRV-ROOM1", IpAddress = "192.168.1.10" }
            });

            Assert.Single(vm.Servers);
            Assert.Equal("192.168.1.10", vm.Servers[0].IpAddress);

            var payload = new ServerStatusPayload
            {
                HostName = "SRV-ROOM1",
                ServerName = "SRV-ROOM1",
                NetworkChannel = "DefaultChannel",
                Version = "1.0.0.0",
                UptimeSeconds = 120
            };
            vm.UpdateServerStatus(payload, "192.168.1.25", isOnline: true);

            Assert.Single(vm.Servers);
            Assert.Equal("192.168.1.25", vm.Servers[0].IpAddress);
            Assert.Equal("Online", vm.Servers[0].Status);
        }

        [Fact]
        public void RegisterDiscoveredServers_AddsNewServersAndSetsLoadingFalse()
        {
            var vm = new MonitorViewModel();
            Assert.True(vm.IsLoading);

            var discovered = new List<DiscoveryResponseMessage>
            {
                new DiscoveryResponseMessage { ServerName = "SRV-1", IpAddress = "10.0.0.1" },
                new DiscoveryResponseMessage { ServerName = "SRV-2", IpAddress = "10.0.0.2" }
            };

            vm.RegisterDiscoveredServers(discovered);

            Assert.Equal(2, vm.Servers.Count);
            Assert.Contains(vm.Servers, s => s.HostName == "SRV-1" && s.IpAddress == "10.0.0.1");
            Assert.Contains(vm.Servers, s => s.HostName == "SRV-2" && s.IpAddress == "10.0.0.2");
            Assert.False(vm.IsLoading);
        }

        [Fact]
        public void FlagUndiscoveredServers_MarksOldNodesOffline()
        {
            var vm = new MonitorViewModel();
            var now = DateTime.UtcNow;

            var nodeOld = new ServerNode { HostName = "SRV-OLD", Status = "Online", LastSeen = now.AddSeconds(-35), Payload = new ServerStatusPayload() };
            var nodeRecent = new ServerNode { HostName = "SRV-RECENT", Status = "Online", LastSeen = now.AddSeconds(-10), Payload = new ServerStatusPayload() };

            vm.Servers.Add(nodeOld);
            vm.Servers.Add(nodeRecent);

            // Undiscovered is empty, so both are technically undiscovered
            vm.FlagUndiscoveredServers(new List<DiscoveryResponseMessage>());

            // nodeOld should be offline (last seen > 30s ago)
            Assert.Equal("Offline", nodeOld.Status);
            Assert.Null(nodeOld.Payload);

            // nodeRecent should still be online (last seen <= 30s ago)
            Assert.Equal("Online", nodeRecent.Status);
            Assert.NotNull(nodeRecent.Payload);
        }

        [Fact]
        public void UpdateServerOffline_SetsOfflineImmediately()
        {
            var vm = new MonitorViewModel();
            var node = new ServerNode { HostName = "SRV-X", Status = "Online", Payload = new ServerStatusPayload() };
            vm.Servers.Add(node);

            vm.UpdateServerOffline("SRV-X", "10.10.10.10");

            Assert.Equal("Offline", node.Status);
            Assert.Equal("10.10.10.10", node.IpAddress);
            Assert.Null(node.Payload);
        }

        [Fact]
        public void UpdateServerStatus_IsOnlineFalse_SetsOffline()
        {
            var vm = new MonitorViewModel();
            var node = new ServerNode { HostName = "SRV-X", Status = "Online", Payload = new ServerStatusPayload() };
            vm.Servers.Add(node);

            var payload = new ServerStatusPayload { HostName = "SRV-X" };
            vm.UpdateServerStatus(payload, "1.1.1.1", isOnline: false);

            Assert.Equal("Offline", node.Status);
            Assert.Null(node.Payload);
        }

        [Fact]
        public void UpdateServerStatus_NormalizesTimestampsToLocal()
        {
            var vm = new MonitorViewModel();
            var node = new ServerNode { HostName = "SRV-X", Status = "Unknown" };
            vm.Servers.Add(node);

            var utcTime = new DateTime(2026, 7, 7, 12, 0, 0, DateTimeKind.Utc);

            var payload = new ServerStatusPayload
            {
                HostName = "SRV-X",
                ActiveClients = new List<ActiveClientInfo>
                {
                    new ActiveClientInfo { Ip = "1.2.3.4", ConnectedSince = utcTime }
                },
                RecentJobs = new List<JobHistoryEntry>
                {
                    new JobHistoryEntry { Document = "doc.pdf", Timestamp = utcTime }
                },
                Errors = new List<ServerErrorEntry>
                {
                    new ServerErrorEntry { Message = "Error", Timestamp = utcTime }
                }
            };

            vm.UpdateServerStatus(payload, "1.2.3.4", isOnline: true);

            var localTimeExpected = utcTime.ToLocalTime();

            Assert.Equal(localTimeExpected, node.Payload!.ActiveClients[0].ConnectedSince);
            Assert.Equal(localTimeExpected, node.Payload.RecentJobs[0].Timestamp);
            Assert.Equal(localTimeExpected, node.Payload.Errors[0].Timestamp);
        }

        [Theory]
        // Normal printer and scanner status -> Online
        [InlineData("online", 0, "available", "Online")]
        // Printer status is error -> Warning
        [InlineData("error", 0, "available", "Warning")]
        // Printer queue length is high (>5) -> Warning
        [InlineData("online", 6, "available", "Warning")]
        // Scanner status is error -> Warning
        [InlineData("online", 0, "error", "Warning")]
        public void UpdateServerStatus_Online_ComputesWarningStatusCorrectly(
            string printerStatus, int queueLength, string scannerStatus, string expectedStatus)
        {
            var vm = new MonitorViewModel();
            var node = new ServerNode { HostName = "SRV-X", Status = "Unknown" };
            vm.Servers.Add(node);

            var payload = new ServerStatusPayload
            {
                HostName = "SRV-X",
                Printers = new List<PrinterStatus>
                {
                    new PrinterStatus { Name = "P1", Status = printerStatus, QueueLength = queueLength }
                },
                Scanners = new List<ScannerStatus>
                {
                    new ScannerStatus { Name = "S1", Status = scannerStatus }
                }
            };

            vm.UpdateServerStatus(payload, "10.0.0.1", isOnline: true);

            Assert.Equal(expectedStatus, node.Status);
        }

        [Fact]
        public void RefreshDisplay_TriggersPropertyChangedForLastRefreshText_AndRefreshesNodes()
        {
            var vm = new MonitorViewModel();
            var node = new ServerNode { HostName = "SRV-X", LastSeen = DateTime.UtcNow.AddMinutes(-5) };
            vm.Servers.Add(node);

            bool lastRefreshTextRaised = false;
            bool nodeLastSeenTextRaised = false;

            vm.PropertyChanged += (s, e) =>
            {
                if (e.PropertyName == nameof(MonitorViewModel.LastRefreshText))
                    lastRefreshTextRaised = true;
            };

            node.PropertyChanged += (s, e) =>
            {
                if (e.PropertyName == nameof(ServerNode.LastSeenText))
                    nodeLastSeenTextRaised = true;
            };

            vm.RefreshDisplay();

            Assert.True(lastRefreshTextRaised);
            Assert.True(nodeLastSeenTextRaised);
        }

        // ── Filtering Tests ───────────────────────────────────────────

        [Fact]
        public void Filtering_FilterTextMatchesHostnameOrIP_CorrectlyFilters()
        {
            var vm = new MonitorViewModel();
            vm.Servers.Add(new ServerNode { HostName = "SRV-ALPHA", IpAddress = "10.0.0.5", Status = "Online" });
            vm.Servers.Add(new ServerNode { HostName = "SRV-BETA", IpAddress = "192.168.1.50", Status = "Offline" });

            vm.FilterText = "";
            var items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Equal(2, items.Count);

            vm.FilterText = "alpha";
            items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Single(items);
            Assert.Equal("SRV-ALPHA", items[0].HostName);

            vm.FilterText = ".1.50";
            items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Single(items);
            Assert.Equal("SRV-BETA", items[0].HostName);
        }

        [Fact]
        public void Filtering_ActiveStatusFilter_CorrectlyFilters()
        {
            var vm = new MonitorViewModel();
            vm.Servers.Add(new ServerNode { HostName = "SRV-1", Status = "Online" });
            vm.Servers.Add(new ServerNode { HostName = "SRV-2", Status = "Warning" });
            vm.Servers.Add(new ServerNode { HostName = "SRV-3", Status = "Offline" });

            vm.ActiveStatusFilter = "All";
            var items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Equal(3, items.Count);

            vm.ActiveStatusFilter = "Online";
            items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Single(items);
            Assert.Equal("SRV-1", items[0].HostName);

            vm.ActiveStatusFilter = "Warning";
            items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Single(items);
            Assert.Equal("SRV-2", items[0].HostName);

            vm.ActiveStatusFilter = "Offline";
            items = vm.FilteredServers.Cast<ServerNode>().ToList();
            Assert.Single(items);
            Assert.Equal("SRV-3", items[0].HostName);
        }

        [Fact]
        public void Sorting_OfflineFirst_PutsOfflineServersAtTop()
        {
            var vm = new MonitorViewModel();
            vm.Servers.Add(new ServerNode { HostName = "SRV-A", Status = "Online" });
            vm.Servers.Add(new ServerNode { HostName = "SRV-B", Status = "Offline" });
            vm.Servers.Add(new ServerNode { HostName = "SRV-C", Status = "Warning" });

            var sorted = vm.FilteredServers.Cast<ServerNode>().ToList();

            Assert.Equal(3, sorted.Count);
            Assert.Equal("SRV-B", sorted[0].HostName); // Offline
            Assert.Equal("SRV-C", sorted[1].HostName); // Warning
            Assert.Equal("SRV-A", sorted[2].HostName); // Online
        }
    }
}
