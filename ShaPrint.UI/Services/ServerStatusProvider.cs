using System;
using System.Collections.Generic;
using System.Linq;
using ShaPrint.Core.Network;

namespace ShaPrint.UI.Services
{
    /// <summary>
    /// The pieces of the running server's state needed to answer a monitor <c>GET_STATUS</c>
    /// query. Implemented by the ServerViewModel on each shell (ShaPrint.UI and — via reference —
    /// ShaPrint.WpfApp), so <see cref="ServerStatusProvider"/> never depends on a ViewModel type.
    /// </summary>
    public interface IServerStatusSource
    {
        DateTime? ServerStartTime { get; }

        string NetworkChannel { get; }

        IReadOnlyCollection<string> ExposedPrinters { get; }

        IReadOnlyCollection<string> ExposedScanners { get; }

        IEnumerable<JobHistoryEntry> RecentJobs { get; }

        IEnumerable<ServerErrorEntry> Errors { get; }

        Dictionary<string, DateTime> GetActiveClientsWithConnectionTimes();
    }

    /// <summary>
    /// Composes a <see cref="ServerStatusPayload"/> for the TCP 9878 monitor channel. Migrated
    /// from <c>ShaPrint.WpfApp/Services/Server/ServerStatusProvider.cs</c> (Gap-closing task) with
    /// the same payload shape and values; the two Windows-only data sources (per-printer queue
    /// status via <c>LocalPrintServer</c>, scanner liveness via <c>ScannerService</c> statics)
    /// are extracted into overridable methods that <see cref="WindowsServerStatusProvider"/>
    /// (#if WINDOWS) supplies. The base implementation is honest-but-minimal so the net8.0 build
    /// (macOS/Linux) still answers 9878 without referencing System.Printing.
    /// </summary>
    public class ServerStatusProvider
    {
        private readonly IServerStatusSource _source;

        /// <summary>Exposed to platform subclasses (e.g. <see cref="WindowsServerStatusProvider"/>)
        /// so they can read the same server state the base payload uses.</summary>
        protected IServerStatusSource Source => _source;

        public ServerStatusProvider(IServerStatusSource source)
        {
            _source = source;
        }

        public ServerStatusPayload BuildStatus()
        {
            var payload = new ServerStatusPayload
            {
                ServerName = Environment.MachineName,
                HostName = Environment.MachineName,
                NetworkChannel = _source.NetworkChannel,
                Version = typeof(ServerStatusProvider).Assembly.GetName().Version?.ToString() ?? "1.0.0.0",
                UptimeSeconds = GetUptimeSeconds()
            };

            // 1. Gather Printer status
            payload.Printers = BuildPrinterStatuses();

            // 2. Gather Scanner status
            payload.Scanners = BuildScannerStatuses();

            // 3. Gather Active Clients
            payload.ActiveClients = BuildActiveClients();

            // 4. Gather Recent Jobs
            payload.RecentJobs = _source.RecentJobs.ToList();

            // 5. Gather Errors
            payload.Errors = _source.Errors.ToList();

            return payload;
        }

        private long GetUptimeSeconds()
        {
            if (_source.ServerStartTime.HasValue)
            {
                var diff = DateTime.UtcNow - _source.ServerStartTime.Value;
                return (long)diff.TotalSeconds;
            }
            return 0;
        }

        /// <summary>
        /// Platform-agnostic default: without a spooler probe we cannot rate a printer, so report
        /// an honest "unknown" instead of fabricating "idle"/"online". Windows overrides this with
        /// real LocalPrintServer data (WpfApp-identical status strings, see
        /// <see cref="WindowsServerStatusProvider"/>).
        /// </summary>
        protected virtual List<PrinterStatus> BuildPrinterStatuses()
        {
            return _source.ExposedPrinters.Select(printerName => new PrinterStatus
            {
                Name = printerName,
                Status = "unknown",
                QueueLength = 0,
                ErrorDescription = null
            }).ToList();
        }

        /// <summary>Same honest default for scanners; Windows overrides with ScannerService statics.</summary>
        protected virtual List<ScannerStatus> BuildScannerStatuses()
        {
            return _source.ExposedScanners.Select(scannerName => new ScannerStatus
            {
                Name = scannerName,
                Status = "available",
                LastScanAgo = null
            }).ToList();
        }

        private List<ActiveClientInfo> BuildActiveClients()
        {
            var activeClients = new List<ActiveClientInfo>();
            foreach (var kvp in _source.GetActiveClientsWithConnectionTimes())
            {
                activeClients.Add(new ActiveClientInfo
                {
                    Ip = kvp.Key,
                    ConnectedSince = kvp.Value
                });
            }
            return activeClients;
        }
    }
}