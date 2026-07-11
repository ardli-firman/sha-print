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
        [NotifyPropertyChangedFor(nameof(StatusTextBrush))]
        [NotifyPropertyChangedFor(nameof(StatusBackgroundBrush))]
        [NotifyPropertyChangedFor(nameof(StatusBorderBrush))]
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
        [NotifyPropertyChangedFor(nameof(Version))]
        [NotifyPropertyChangedFor(nameof(NetworkChannel))]
        private ServerStatusPayload? _payload;

        [ObservableProperty]
        private int _selectedTabIndex;

        public string Version => Payload?.Version ?? "Unknown";
        public string NetworkChannel => Payload?.NetworkChannel ?? "Unknown";

        public int StatusSortOrder => Status switch
        {
            "Offline" => 1,
            "AuthMismatch" => 2,
            "Unreachable" => 3,
            "Warning" => 4,
            "Online" => 5,
            _ => 6
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
                        "AuthMismatch" => "SystemFillColorCriticalBrush",
                        "Unreachable" => "SystemFillColorCautionBrush",
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
                        "AuthMismatch" => Brushes.Red,
                        "Unreachable" => Brushes.Orange,
                        _ => Brushes.Gray
                    };
                }
            }
        }

        public Brush StatusTextBrush => StatusBrush;

        public Brush StatusBackgroundBrush => GetModifiedOpacityBrush(StatusBrush, 26); // ~10% opacity

        public Brush StatusBorderBrush => GetModifiedOpacityBrush(StatusBrush, 76); // ~30% opacity

        private Brush GetModifiedOpacityBrush(Brush baseBrush, byte opacity)
        {
            if (baseBrush is SolidColorBrush solidBrush)
            {
                return new SolidColorBrush(Color.FromArgb(opacity, solidBrush.Color.R, solidBrush.Color.G, solidBrush.Color.B));
            }
            return baseBrush;
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

        private string _activeStatusFilter = "All";
        public string ActiveStatusFilter
        {
            get => _activeStatusFilter;
            set
            {
                if (SetProperty(ref _activeStatusFilter, value))
                {
                    SafeRefreshView();
                }
            }
        }

        [ObservableProperty] private int _totalServers;
        [ObservableProperty] private int _onlineCount;
        [ObservableProperty] private int _warningCount;
        [ObservableProperty] private int _unreachableCount;
        [ObservableProperty] private int _authMismatchCount;
        [ObservableProperty] private int _offlineCount;
        [ObservableProperty] private int _unknownCount;
        [ObservableProperty] private bool _hasAnyErrors;
        [ObservableProperty] private bool _isWeakChannel;
        [ObservableProperty] private bool _isEmpty = true;
        [ObservableProperty] private bool _isFilterNoResults;
        [ObservableProperty] private bool _isLoading = true;
        
        [ObservableProperty] 
        [NotifyPropertyChangedFor(nameof(LastRefreshText))]
        private DateTime? _lastRefreshTime;

        public string LastRefreshText
        {
            get
            {
                if (LastRefreshTime == null) return "Never";
                var diff = DateTime.UtcNow - LastRefreshTime.Value;
                if (diff.TotalSeconds < 5) return "Just now";
                if (diff.TotalSeconds < 60) return $"{(int)diff.TotalSeconds}s ago";
                if (diff.TotalMinutes < 60) return $"{(int)diff.TotalMinutes}m ago";
                return $"{(int)diff.TotalHours}h ago";
            }
        }

        public IAsyncRelayCommand? RefreshCommand { get; set; }

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
                UpdateSummary();
            }
            catch (Exception ex) when (ex is NotSupportedException || ex is InvalidOperationException)
            {
                // Ignore threading exceptions in headless tests
            }
        }

        private void UpdateSummary()
        {
            TotalServers = Servers.Count;
            OnlineCount = Servers.Count(s => s.Status == "Online");
            WarningCount = Servers.Count(s => s.Status == "Warning");
            UnreachableCount = Servers.Count(s => s.Status == "Unreachable");
            AuthMismatchCount = Servers.Count(s => s.Status == "AuthMismatch");
            OfflineCount = Servers.Count(s => s.Status == "Offline");
            UnknownCount = Servers.Count(s => s.Status == "Unknown");
            HasAnyErrors = Servers.Any(s => s.Payload?.Errors?.Count > 0);
            IsWeakChannel = ShaPrint.WpfApp.Models.AppSettings.Current.NetworkChannel == "DefaultChannel" || 
                            string.IsNullOrWhiteSpace(ShaPrint.WpfApp.Models.AppSettings.Current.NetworkChannel) || 
                            ShaPrint.WpfApp.Models.AppSettings.Current.NetworkChannel.Trim().Length < 8;
            IsEmpty = Servers.Count == 0;
            
            // If we have servers but filtered view is empty, and filter is not empty
            IsFilterNoResults = Servers.Count > 0 && !IsFilterEmpty && _serversView.Cast<object>().Count() == 0;
        }

        [RelayCommand]
        private void ToggleExpand(ServerNode node)
        {
            if (node != null)
            {
                node.IsExpanded = !node.IsExpanded;
            }
        }

        [RelayCommand]
        private void ExpandAll()
        {
            foreach (var server in Servers)
            {
                server.IsExpanded = true;
            }
        }

        [RelayCommand]
        private void CollapseAll()
        {
            foreach (var server in Servers)
            {
                server.IsExpanded = false;
            }
        }

        [RelayCommand]
        private void SetStatusFilter(string status)
        {
            if (!string.IsNullOrEmpty(status))
            {
                ActiveStatusFilter = status;
            }
        }


        private bool FilterServerNode(object item)
        {
            if (item is ServerNode node)
            {
                if (ActiveStatusFilter != "All" && !node.Status.Equals(ActiveStatusFilter, StringComparison.OrdinalIgnoreCase))
                {
                    return false;
                }

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
                IsLoading = false;
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
                    // Convert UTC timestamps to local time for proper UI display
                    if (payload.ActiveClients != null)
                    {
                        foreach (var client in payload.ActiveClients)
                        {
                            if (client.ConnectedSince.Kind != DateTimeKind.Local)
                            {
                                var utcTime = client.ConnectedSince.Kind == DateTimeKind.Utc 
                                    ? client.ConnectedSince 
                                    : DateTime.SpecifyKind(client.ConnectedSince, DateTimeKind.Utc);
                                client.ConnectedSince = utcTime.ToLocalTime();
                            }
                        }
                    }
                    if (payload.RecentJobs != null)
                    {
                        foreach (var job in payload.RecentJobs)
                        {
                            if (job.Timestamp.Kind != DateTimeKind.Local)
                            {
                                var utcTime = job.Timestamp.Kind == DateTimeKind.Utc 
                                    ? job.Timestamp 
                                    : DateTime.SpecifyKind(job.Timestamp, DateTimeKind.Utc);
                                job.Timestamp = utcTime.ToLocalTime();
                            }
                        }
                    }
                    if (payload.Errors != null)
                    {
                        foreach (var err in payload.Errors)
                        {
                            if (err.Timestamp.Kind != DateTimeKind.Local)
                            {
                                var utcTime = err.Timestamp.Kind == DateTimeKind.Utc 
                                    ? err.Timestamp 
                                    : DateTime.SpecifyKind(err.Timestamp, DateTimeKind.Utc);
                                err.Timestamp = utcTime.ToLocalTime();
                            }
                        }
                    }

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

        public void UpdateServerFailure(string hostName, string ipAddress, string category)
        {
            RunOnDispatcher(() =>
            {
                var server = Servers.FirstOrDefault(s => s.HostName.Equals(hostName, StringComparison.OrdinalIgnoreCase));
                if (server != null)
                {
                    server.IpAddress = ipAddress;

                    if (category == "AuthMismatch")
                    {
                        server.Status = "AuthMismatch";
                        server.Payload = null;
                    }
                    else
                    {
                        if (DateTime.UtcNow - server.LastSeen > TimeSpan.FromSeconds(30))
                        {
                            server.Status = "Offline";
                            server.Payload = null;
                        }
                        else
                        {
                            server.Status = "Unreachable";
                        }
                    }
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
                OnPropertyChanged(nameof(LastRefreshText));
            });
        }
    }
}
