using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Linq;
using System.Windows;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core.Network;

namespace ShaPrint.WpfApp.ViewModels.Pages
{
    public partial class ServerNode : ObservableObject
    {
        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(StatusBrush))]
        [NotifyPropertyChangedFor(nameof(StatusSortOrder))]
        private string _status = "Unknown"; // "Online", "Warning", "Offline", "Unknown"

        [ObservableProperty]
        private string _hostName = string.Empty;

        [ObservableProperty]
        private string _ipAddress = string.Empty;

        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(UptimeText))]
        private long _uptimeSeconds;

        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(LastSeenText))]
        private DateTime _lastSeen = DateTime.UtcNow;

        [ObservableProperty]
        private bool _isExpanded;

        [ObservableProperty]
        private ServerStatusPayload? _payload;

        public int StatusSortOrder => Status switch
        {
            "Offline" => 1,
            "Warning" => 2,
            "Online" => 3,
            _ => 4
        };

        public Brush StatusBrush
        {
            get
            {
                try
                {
                    string resourceKey = Status switch
                    {
                        "Online" => "SystemFillColorSuccessBrush",
                        "Warning" => "SystemFillColorCautionBrush",
                        "Offline" => "SystemFillColorCriticalBrush",
                        _ => "TextFillColorDisabledBrush"
                    };
                    return (Brush)Application.Current.FindResource(resourceKey);
                }
                catch
                {
                    // Fallback if resource not found
                    return Status switch
                    {
                        "Online" => Brushes.Green,
                        "Warning" => Brushes.Orange,
                        "Offline" => Brushes.Red,
                        _ => Brushes.Gray
                    };
                }
            }
        }

        public string UptimeText
        {
            get
            {
                if (UptimeSeconds <= 0) return "0s";
                var span = TimeSpan.FromSeconds(UptimeSeconds);
                if (span.TotalDays >= 1)
                    return $"{(int)span.TotalDays}d {span.Hours}h {span.Minutes}m";
                if (span.TotalHours >= 1)
                    return $"{span.Hours}h {span.Minutes}m {span.Seconds}s";
                return $"{span.Minutes}m {span.Seconds}s";
            }
        }

        public string LastSeenText
        {
            get
            {
                var diff = DateTime.UtcNow - LastSeen;
                if (diff.TotalSeconds < 5) return "Just now";
                if (diff.TotalSeconds < 60) return $"{(int)diff.TotalSeconds}s ago";
                if (diff.TotalMinutes < 60) return $"{(int)diff.TotalMinutes}m ago";
                return $"{(int)diff.TotalHours}h ago";
            }
        }

        public void RefreshDisplayProperties()
        {
            OnPropertyChanged(nameof(LastSeenText));
        }
    }

    public partial class MonitorViewModel : ObservableObject
    {
        private readonly ICollectionView _serversView;
        private string _filterText = string.Empty;
        private bool _isFilterEmpty = true;
        private static readonly object _serversLock = new();

        public ObservableCollection<ServerNode> Servers { get; } = new();

        public ICollectionView FilteredServers => _serversView;

        public string FilterText
        {
            get => _filterText;
            set
            {
                if (SetProperty(ref _filterText, value))
                {
                    IsFilterEmpty = string.IsNullOrWhiteSpace(value);
                    SafeRefreshView();
                }
            }
        }

        public bool IsFilterEmpty
        {
            get => _isFilterEmpty;
            private set => SetProperty(ref _isFilterEmpty, value);
        }

        public MonitorViewModel()
        {
            BindingOperations.EnableCollectionSynchronization(Servers, _serversLock);

            _serversView = CollectionViewSource.GetDefaultView(Servers);
            _serversView.Filter = FilterServerNode;

            // Sort descriptions: Offline first (SortOrder 1), then Warning (2), then Online (3), then Unknown (4)
            // Secondary sort: HostName
            _serversView.SortDescriptions.Add(new SortDescription(nameof(ServerNode.StatusSortOrder), ListSortDirection.Ascending));
            _serversView.SortDescriptions.Add(new SortDescription(nameof(ServerNode.HostName), ListSortDirection.Ascending));
        }

        private void SafeRefreshView()
        {
            try
            {
                _serversView?.Refresh();
            }
            catch (Exception ex) when (ex is NotSupportedException || ex is InvalidOperationException)
            {
                // Ignore threading exceptions in headless tests
            }
        }

        [RelayCommand]
        private void ToggleExpand(ServerNode node)
        {
            if (node != null)
            {
                node.IsExpanded = !node.IsExpanded;
            }
        }


        private bool FilterServerNode(object item)
        {
            if (item is ServerNode node)
            {
                if (string.IsNullOrWhiteSpace(FilterText))
                    return true;

                string query = FilterText.Trim();
                return node.HostName.Contains(query, StringComparison.OrdinalIgnoreCase) ||
                       node.IpAddress.Contains(query, StringComparison.OrdinalIgnoreCase);
            }
            return false;
        }

        private void RunOnDispatcher(Action action)
        {
            if (Application.Current?.Dispatcher != null)
            {
                if (Application.Current.Dispatcher.CheckAccess())
                {
                    action();
                }
                else
                {
                    Application.Current.Dispatcher.Invoke(action);
                }
            }
            else
            {
                action();
            }
        }

        public void RegisterDiscoveredServers(List<DiscoveryResponseMessage> discovered)
        {
            RunOnDispatcher(() =>
            {
                var currentNames = Servers.Select(s => s.HostName).ToList();

                foreach (var server in discovered)
                {
                    var existing = Servers.FirstOrDefault(s => s.HostName.Equals(server.ServerName, StringComparison.OrdinalIgnoreCase));
                    if (existing == null)
                    {
                        Servers.Add(new ServerNode
                        {
                            HostName = server.ServerName,
                            IpAddress = server.IpAddress,
                            Status = "Unknown",
                            LastSeen = DateTime.UtcNow
                        });
                    }
                    else
                    {
                        // Update IP if it changed (DHCP resilience)
                        if (existing.IpAddress != server.IpAddress)
                        {
                            existing.IpAddress = server.IpAddress;
                        }
                    }
                }
                SafeRefreshView();
            });
        }

        public void FlagUndiscoveredServers(List<DiscoveryResponseMessage> discovered)
        {
            RunOnDispatcher(() =>
            {
                var discoveredNames = discovered.Select(d => d.ServerName).ToList();
                foreach (var server in Servers)
                {
                    if (!discoveredNames.Contains(server.HostName, StringComparer.OrdinalIgnoreCase))
                    {
                        // If it wasn't discovered in this UDP cycle, and was last seen > 30s ago, mark Offline
                        if (DateTime.UtcNow - server.LastSeen > TimeSpan.FromSeconds(30))
                        {
                            server.Status = "Offline";
                            server.Payload = null;
                        }
                    }
                }
                SafeRefreshView();
            });
        }

        public void UpdateServerStatus(ServerStatusPayload payload, string ipAddress, bool isOnline)
        {
            RunOnDispatcher(() =>
            {
                var server = Servers.FirstOrDefault(s => s.HostName.Equals(payload.HostName, StringComparison.OrdinalIgnoreCase));
                if (server != null)
                {
                    server.IpAddress = ipAddress;
                    server.UptimeSeconds = payload.UptimeSeconds;
                    server.LastSeen = DateTime.UtcNow;
                    server.Payload = payload;

                    if (isOnline)
                    {
                        // Check warning conditions: queue length > 5, scanner error, or printer error
                        bool hasWarning = payload.Printers.Any(p => p.Status == "error" || p.QueueLength > 5) ||
                                          payload.Scanners.Any(s => s.Status == "error");

                        server.Status = hasWarning ? "Warning" : "Online";
                    }
                    else
                    {
                        server.Status = "Offline";
                        server.Payload = null;
                    }
                }
                SafeRefreshView();
            });
        }

        public void UpdateServerOffline(string hostName, string ipAddress)
        {
            RunOnDispatcher(() =>
            {
                var server = Servers.FirstOrDefault(s => s.HostName.Equals(hostName, StringComparison.OrdinalIgnoreCase));
                if (server != null)
                {
                    // Only mark offline if last seen was > 30s ago, or if TCP connection failed (TCP failure is immediate)
                    server.Status = "Offline";
                    server.IpAddress = ipAddress;
                    server.Payload = null;
                }
                SafeRefreshView();
            });
        }

        public void RefreshDisplay()
        {
            RunOnDispatcher(() =>
            {
                foreach (var server in Servers)
                {
                    server.RefreshDisplayProperties();
                }
            });
        }
    }
}
