using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.ViewModels.Pages;

namespace ShaPrint.Client
{
    public sealed class ServerIdentityChangedArgs : EventArgs
    {
        public required string ServerId { get; init; }
        public required string? ServerName { get; init; }
        public required string OldIp { get; init; }
        public required string NewIp { get; init; }
        public required string PipeName { get; init; }
        public required string VirtualPrinterName { get; init; }
    }

    public sealed class SuspiciousMatchArgs : EventArgs
    {
        public required string ServerId { get; init; }
        public required string ExpectedServerName { get; init; }
        public required string PipeName { get; init; }
    }

    public sealed class ServerReachabilityTracker
    {
        public enum RescanReason { Startup, ClientPageOpen, PrintFailed }

        private readonly Func<IReadOnlyList<InstalledPrinterConfig>> _configProvider;
        private readonly Func<Task<List<DiscoveryResponseMessage>>> _scanner;
        private readonly Action<ServerIdentityChangedArgs> _onIdentityChanged;
        private readonly Action<SuspiciousMatchArgs> _onSuspiciousMatch;
        private readonly TimeSpan _debounceWindow;
        private readonly SemaphoreSlim _scanGate = new(1, 1);
        private DateTime _lastGlobalScan = DateTime.MinValue;

        public ServerReachabilityTracker(
            Func<IReadOnlyList<InstalledPrinterConfig>> configProvider,
            Func<Task<List<DiscoveryResponseMessage>>> scanner,
            Action<ServerIdentityChangedArgs> onIdentityChanged,
            Action<SuspiciousMatchArgs> onSuspiciousMatch,
            TimeSpan? debounceWindow = null)
        {
            _configProvider = configProvider;
            _scanner = scanner;
            _onIdentityChanged = onIdentityChanged;
            _onSuspiciousMatch = onSuspiciousMatch;
            _debounceWindow = debounceWindow ?? TimeSpan.FromSeconds(30);
        }

        public async Task RequestRescanAsync(RescanReason reason, CancellationToken ct)
        {
            var now = DateTime.UtcNow;
            if (now - _lastGlobalScan < _debounceWindow)
            {
                ShaPrint.Core.AppLogger.Log($"[TRACKER] Rescan debounced. Reason: {reason}");
                return;
            }

            await _scanGate.WaitAsync(ct);
            try
            {
                // Double-check after acquiring the semaphore lock
                if (DateTime.UtcNow - _lastGlobalScan < _debounceWindow)
                {
                    return;
                }

                ShaPrint.Core.AppLogger.Log($"[TRACKER] Requesting rescan. Reason: {reason}");
                var results = await _scanner().ConfigureAwait(false);
                _lastGlobalScan = DateTime.UtcNow;
                ApplyMatches(results);
            }
            finally
            {
                _scanGate.Release();
            }
        }

        private void ApplyMatches(IReadOnlyList<DiscoveryResponseMessage> results)
        {
            foreach (var cfg in _configProvider().ToList())
            {
                if (FindMatch(cfg, results) is not { } match) continue;

                if (match.IdentityMismatch)
                {
                    _onSuspiciousMatch(new SuspiciousMatchArgs
                    {
                        ServerId = match.Response.ServerId ?? cfg.ServerId ?? "",
                        ExpectedServerName = cfg.VirtualPrinterName,
                        PipeName = cfg.PipeName
                    });
                    continue;
                }

                if (!string.Equals(match.Response.IpAddress, cfg.ServerIp, StringComparison.Ordinal))
                {
                    var oldIp = cfg.ServerIp;
                    cfg.ServerIp = match.Response.IpAddress;
                    _onIdentityChanged(new ServerIdentityChangedArgs
                    {
                        ServerId = cfg.ServerId ?? match.Response.ServerId ?? "",
                        ServerName = match.Response.ServerName,
                        OldIp = oldIp,
                        NewIp = cfg.ServerIp,
                        PipeName = cfg.PipeName,
                        VirtualPrinterName = cfg.VirtualPrinterName
                    });
                }
            }
        }

        private static (DiscoveryResponseMessage Response, bool IdentityMismatch)? FindMatch(
            InstalledPrinterConfig cfg, IReadOnlyList<DiscoveryResponseMessage> results)
        {
            if (!string.IsNullOrEmpty(cfg.ServerId))
            {
                var byId = results.FirstOrDefault(r =>
                    !string.IsNullOrEmpty(r.ServerId) &&
                    string.Equals(r.ServerId, cfg.ServerId, StringComparison.Ordinal));
                if (byId != null) return (byId, false);

                var byName = results.FirstOrDefault(r =>
                    string.Equals(r.ServerName, ExtractServerName(cfg.VirtualPrinterName), StringComparison.OrdinalIgnoreCase));
                if (byName != null) return (byName, true);
                return null;
            }

            var nameFallback = results.FirstOrDefault(r =>
                string.Equals(r.ServerName, ExtractServerName(cfg.VirtualPrinterName), StringComparison.OrdinalIgnoreCase));
            if (nameFallback != null) return (nameFallback, false);
            return null;
        }

        public static string ExtractServerName(string virtualPrinterName)
        {
            const string prefix = "ShaPrint [";
            int start = virtualPrinterName.IndexOf(prefix, StringComparison.Ordinal);
            if (start < 0) return virtualPrinterName;
            int begin = start + prefix.Length;
            int end = virtualPrinterName.IndexOf(']', begin);
            if (end < 0) return virtualPrinterName;
            return virtualPrinterName.Substring(begin, end - begin);
        }
    }
}
