using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core.Network;
using ShaPrint.UI.Models;
using ShaPrint.UI.Services;
using System.Collections.ObjectModel;

namespace ShaPrint.UI.ViewModels.Pages;

/// <summary>
/// One monitored server node. Bindings (Task 6) only surface <c>ServerStatusPayload</c> members:
/// <c>HostName</c>, <c>Version</c>, <c>NetworkChannel</c>, <c>UptimeSeconds</c> (formatted via a
/// view converter), <c>Printers.Count</c>, <c>ActiveClients.Count</c>. The WPF
/// <c>Brush</c>-based status coloring is dropped (view concern now).
/// </summary>
public partial class ServerNode : ObservableObject
{
    [ObservableProperty]
    private string _status = "Unknown"; // "Online", "Warning", "Offline", "Unknown", "Unreachable", "AuthMismatch"

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
    [NotifyPropertyChangedFor(nameof(UptimeText))]
    [NotifyPropertyChangedFor(nameof(Printers))]
    [NotifyPropertyChangedFor(nameof(ActiveClients))]
    private ServerStatusPayload? _payload;

    public string Version => Payload?.Version ?? "Unknown";
    public string NetworkChannel => Payload?.NetworkChannel ?? "Unknown";

    /// <summary>Empty (not null) when offline so <c>Printers.Count</c> bindings stay safe.</summary>
    public IReadOnlyList<PrinterStatus> Printers => Payload?.Printers ?? new List<PrinterStatus>();
    public IReadOnlyList<ActiveClientInfo> ActiveClients => Payload?.ActiveClients ?? new List<ActiveClientInfo>();

    public int StatusSortOrder => Status switch
    {
        "Offline" => 1,
        "AuthMismatch" => 2,
        "Unreachable" => 3,
        "Warning" => 4,
        "Online" => 5,
        _ => 6
    };

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

/// <summary>
/// Monitor mode page. Migrated from <c>ShaPrint.WpfApp/ViewModels/Pages/MonitorViewModel.cs</c>
/// (Task 5). The WPF <c>MonitorService</c> called into the VM directly; the shared
/// <see cref="Services.MonitorService"/> (Task 4) is event-based, and this VM subscribes to
/// <c>ServersDiscovered</c> / <c>ServersReconciled</c> / <c>ServerStatusUpdated</c> /
/// <c>ServerFailure</c> — no self-polling, no service-side refresh command.
///
/// WPF -> Avalonia adaptations: <c>ICollectionView</c> filtering/sorting is replaced by a
/// rebuilt <see cref="FilteredServers"/> collection; WPF brushes are dropped; the
/// <c>RefreshCommand</c> property is now a real <see cref="RefreshAsyncCommand"/> calling
/// <see cref="Services.MonitorService.TriggerManualRefreshAsync"/>.
/// </summary>
public partial class MonitorViewModel : ObservableObject, IDisposable
{
    private readonly MonitorService _monitorService;
    private string _filterText = string.Empty;
    private bool _isFilterEmpty = true;
    private string _activeStatusFilter = "All";

    public ObservableCollection<ServerNode> Servers { get; } = new();

    /// <summary>Filtered/sorted projection rebuilt on every change (replaces WPF ICollectionView).</summary>
    public ObservableCollection<ServerNode> FilteredServers { get; } = new();

    public string FilterText
    {
        get => _filterText;
        set
        {
            if (SetProperty(ref _filterText, value))
            {
                IsFilterEmpty = string.IsNullOrWhiteSpace(value);
                RefreshView();
            }
        }
    }

    public bool IsFilterEmpty
    {
        get => _isFilterEmpty;
        private set => SetProperty(ref _isFilterEmpty, value);
    }

    public string ActiveStatusFilter
    {
        get => _activeStatusFilter;
        set
        {
            if (SetProperty(ref _activeStatusFilter, value))
            {
                RefreshView();
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

    public MonitorViewModel(MonitorService monitorService)
    {
        _monitorService = monitorService;

        _monitorService.ServersDiscovered += OnServersDiscovered;
        _monitorService.ServersReconciled += OnServersReconciled;
        _monitorService.ServerStatusUpdated += OnServerStatusUpdated;
        _monitorService.ServerFailure += OnServerFailure;
    }

    /// <summary>Starts the shared polling service (idempotent). Called when entering Monitor mode
    /// (WelcomeViewModel) — same lifecycle as WpfApp App.OnStartup.</summary>
    public void Start() => _monitorService.Start();

    public void Stop() => _monitorService.Stop();

    [RelayCommand]
    private async Task RefreshAsync() => await _monitorService.TriggerManualRefreshAsync();

    // ── Event handlers (raised from the service's poll loop) ─────────────────

    private void OnServersDiscovered(object? sender, MonitorServersDiscoveredEventArgs e)
        => RunOnDispatcher(() => RegisterDiscoveredServers(e.Servers));

    private void OnServersReconciled(object? sender, MonitorServersReconciledEventArgs e)
        => RunOnDispatcher(() =>
        {
            LastRefreshTime = e.LastRefreshTime;
            FlagUndiscoveredServers(e.Servers);
        });

    private void OnServerStatusUpdated(object? sender, MonitorServerStatusUpdatedEventArgs e)
        => RunOnDispatcher(() => UpdateServerStatus(e.Payload, e.IpAddress));

    private void OnServerFailure(object? sender, MonitorServerFailureEventArgs e)
        => RunOnDispatcher(() => UpdateServerFailure(e.ServerName, e.IpAddress, e.Reason));

    // ── UI mutations (same logic as WpfApp) ──────────────────────────────────

    private void RefreshView()
    {
        UpdateSummary();
        RebuildFilteredServers();
    }

    private void RebuildFilteredServers()
    {
        IEnumerable<ServerNode> query = Servers;

        if (ActiveStatusFilter != "All")
        {
            query = query.Where(n => n.Status.Equals(ActiveStatusFilter, StringComparison.OrdinalIgnoreCase));
        }

        if (!string.IsNullOrWhiteSpace(FilterText))
        {
            string q = FilterText.Trim();
            query = query.Where(n =>
                n.HostName.Contains(q, StringComparison.OrdinalIgnoreCase) ||
                n.IpAddress.Contains(q, StringComparison.OrdinalIgnoreCase));
        }

        var filtered = query
            .OrderBy(n => n.StatusSortOrder)
            .ThenBy(n => n.HostName, StringComparer.OrdinalIgnoreCase)
            .ToList();

        FilteredServers.Clear();
        foreach (var node in filtered)
        {
            FilteredServers.Add(node);
        }

        // If we have servers but filtered view is empty, and filter is not empty
        IsFilterNoResults = Servers.Count > 0 && !IsFilterEmpty && filtered.Count == 0;
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
        IsWeakChannel = AppSettings.Current.NetworkChannel == "DefaultChannel" ||
                        string.IsNullOrWhiteSpace(AppSettings.Current.NetworkChannel) ||
                        AppSettings.Current.NetworkChannel.Trim().Length < 8;
        IsEmpty = Servers.Count == 0;
    }

    [RelayCommand]
    private void ToggleExpand(ServerNode node)
    {
        if (node != null)
        {
            node.IsExpanded = !node.IsExpanded;
            RunOnDispatcher(RefreshView);
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

    private void RunOnDispatcher(Action action)
    {
        if (Avalonia.Application.Current is not null)
        {
            if (Dispatcher.UIThread.CheckAccess())
            {
                action();
            }
            else
            {
                Dispatcher.UIThread.Post(action);
            }
        }
        else
        {
            action();
        }
    }

    public void RegisterDiscoveredServers(IReadOnlyList<DiscoveryResponseMessage> discovered)
    {
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
        RefreshView();
    }

    public void FlagUndiscoveredServers(IReadOnlyList<DiscoveryResponseMessage> discovered)
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
        RefreshView();
    }

    public void UpdateServerStatus(ServerStatusPayload payload, string ipAddress)
    {
        var server = Servers.FirstOrDefault(s => s.HostName.Equals(payload.HostName, StringComparison.OrdinalIgnoreCase));
        if (server != null)
        {
            server.IpAddress = ipAddress;
            server.UptimeSeconds = payload.UptimeSeconds;
            server.LastSeen = DateTime.UtcNow;
            server.Payload = payload;

            // Check warning conditions: queue length > 5, scanner error, or printer error
            bool hasWarning = payload.Printers.Any(p => p.Status == "error" || p.QueueLength > 5) ||
                              payload.Scanners.Any(s => s.Status == "error");

            server.Status = hasWarning ? "Warning" : "Online";
        }
        RefreshView();
    }

    public void UpdateServerFailure(string hostName, string ipAddress, string category)
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
        RefreshView();
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

    public void Dispose()
    {
        _monitorService.ServersDiscovered -= OnServersDiscovered;
        _monitorService.ServersReconciled -= OnServersReconciled;
        _monitorService.ServerStatusUpdated -= OnServerStatusUpdated;
        _monitorService.ServerFailure -= OnServerFailure;
        Stop();
    }
}