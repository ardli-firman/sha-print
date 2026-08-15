using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Abstractions;

namespace ShaPrint.Server
{
    /// <summary>
    /// Monitors PrintService event log for driver change events (event ID 312)
    /// and invalidates the DriverPackageService cache accordingly.
    /// Also enforces TTL-based invalidation as a fallback.
    /// </summary>
    public class DriverCacheInvalidator : IDisposable
    {
        private readonly DriverPackageService _driverPackageService;
        private readonly IEventLog _eventLog;
        private readonly TimeSpan _ttl;
        private CancellationTokenSource? _cts;
        private DateTime _lastInvalidationTime = DateTime.UtcNow;

        /// <summary>
        /// PrintService operational log event ID for driver changes.
        /// </summary>
        private const int DriverChangedEventId = 312;
        private const string PrintServiceLogName = "Microsoft-Windows-PrintService/Operational";

        public DriverCacheInvalidator(
            DriverPackageService driverPackageService,
            IEventLog eventLog,
            TimeSpan? ttl = null)
        {
            _driverPackageService = driverPackageService;
            _eventLog = eventLog;
            _ttl = ttl ?? TimeSpan.FromHours(Constants.DriverPackageCacheTtlHours);
        }

        /// <summary>
        /// Starts the background monitoring loop.
        /// Polls event log every 60 seconds for driver change events.
        /// </summary>
        public void Start()
        {
            _cts = new CancellationTokenSource();
            Task.Run(() => MonitorLoopAsync(_cts.Token));
        }

        public void Stop()
        {
            _cts?.Cancel();
        }

        public void Dispose()
        {
            Stop();
            _cts?.Dispose();
        }

        private async Task MonitorLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    await Task.Delay(TimeSpan.FromSeconds(60), token);

                    // Check for driver change events since last check
                    try
                    {
                        var entries = _eventLog.GetEntries(PrintServiceLogName, DriverChangedEventId);
                        var recentEvent = entries
                            .Where(e => e.TimeGenerated > _lastInvalidationTime)
                            .OrderByDescending(e => e.TimeGenerated)
                            .FirstOrDefault();

                        if (recentEvent != null)
                        {
                            AppLogger.Log($"[DRIVER_CACHE] Driver change event detected (ID={recentEvent.EventId}, time={recentEvent.TimeGenerated}). Invalidating cache.");
                            _driverPackageService.InvalidateAll();
                            _lastInvalidationTime = DateTime.UtcNow;
                        }
                    }
                    catch (Exception ex)
                    {
                        // Event log access may fail on some systems — not fatal
                        AppLogger.Log($"[DRIVER_CACHE] Event log check failed (non-fatal): {ex.Message}");
                    }

                    // TTL fallback: if no events but cache is stale, the GetDriverAsync
                    // already handles staleness via IsCacheStale() — no extra action needed here.
                }
                catch (OperationCanceledException) { break; }
                catch (Exception ex)
                {
                    AppLogger.Error("[DRIVER_CACHE] Unexpected error in monitor loop", ex);
                }
            }
        }
    }
}
