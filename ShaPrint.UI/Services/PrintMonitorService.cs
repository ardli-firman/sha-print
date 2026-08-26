using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Platform.Abstractions;
using ShaPrint.UI.Models;

namespace ShaPrint.UI.Services
{
    /// <summary>
    /// Record kept in the dedup dictionary for every jobId the monitor has
    /// already cancelled + alerted on. Used to suppress repeat alerts and
    /// to feed the log path.
    /// </summary>
    public record IncidentRecord(string PrinterName, string JobName, DateTime FirstSeenUtc);

    /// <summary>
    /// Raised on <see cref="PrintMonitorService.AlertRaised"/> when a stuck hard-error job is
    /// auto-purged. The WpfApp monitor pushed the same information to a snackbar + toast;
    /// the shared service surfaces it as an event so each shell decides how to present it.
    /// </summary>
    public sealed record PrintMonitorAlert(string PrinterName, JobSnapshot Job, string Message);

    /// <summary>
    /// Background auto-purge monitor for the server's exposed printers. Migrated from
    /// <c>ShaPrint.WpfApp/Services/Server/PrintMonitorService.cs</c> (Gap-closing task) as an
    /// injectable, platform-agnostic service:
    /// <list type="bullet">
    /// <item>Spooler access goes exclusively through <see cref="IPrintQueueProbe"/> — the real
    /// Windows implementation (<see cref="LocalPrintQueueProbe"/>) is compiled only under
    /// #if WINDOWS, so this class stays runnable on net8.0.</item>
    /// <item>Alerts are event-based (<see cref="AlertRaised"/>) instead of a WPF snackbar; the
    /// error toast still goes through <see cref="INotificationService"/>.</item>
    /// <item>AutoPurge setting is read from <see cref="Models.AppSettings"/> by default; the WpfApp
    /// shell injects a delegate reading its own AppSettings so its runtime toggle keeps working.</item>
    /// </list>
    /// Dedup/streak/eviction logic is unchanged from WpfApp.
    /// </summary>
    public class PrintMonitorService
    {
        private const int StreakCap = 10;

        private CancellationTokenSource? _cts;
        private readonly INotificationService _notificationService;
        private readonly IPrintQueueProbe _probe;
        private readonly IDelayProbe _delay;
        private readonly Func<bool> _isAutoPurgeEnabled;
        private List<string> _monitoredPrinters = new List<string>();

        // Per-jobId: how many consecutive polls it has been in a hard error state.
        private readonly ConcurrentDictionary<int, int> _hardErrorStreak = new();

        // Per-jobId: the set of incidents we have already cancelled + alerted on.
        private readonly ConcurrentDictionary<int, IncidentRecord> _seenIncidents = new();

        /// <summary>Fired (on the monitor loop thread) after a job is auto-purged.</summary>
        public event Action<PrintMonitorAlert>? AlertRaised;

        public PrintMonitorService(
            INotificationService notificationService,
            IPrintQueueProbe probe,
            IDelayProbe delay,
            Func<bool>? isAutoPurgeEnabled = null)
        {
            _notificationService = notificationService;
            _probe = probe;
            _delay = delay;
            _isAutoPurgeEnabled = isAutoPurgeEnabled
                ?? (() => AppSettings.Current.AutoPurgeEnabled);
        }

        public void SetMonitoredPrinters(List<string> printers)
        {
            _monitoredPrinters = printers ?? new List<string>();
        }

        public void Start()
        {
            if (_cts != null) return;
            _cts = new CancellationTokenSource();
            _ = Task.Run(() => MonitorLoopAsync(_cts.Token));
        }

        public void Stop()
        {
            if (_cts == null) return;
            _cts.Cancel();
            _cts = null;
        }

        public static bool IsHardError(MonitorJobStatus status)
        {
            return status.HasFlag(MonitorJobStatus.Error) ||
                   status.HasFlag(MonitorJobStatus.PaperOut) ||
                   status.HasFlag(MonitorJobStatus.Blocked);
        }

        private async Task MonitorLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    if (_isAutoPurgeEnabled())
                    {
                        await CheckPrintQueuesAsync(token);
                    }
                }
                catch (Exception ex)
                {
                    AppLogger.Error("[MONITOR] Loop iteration failed", ex);
                }

                await _delay.Delay(TimeSpan.FromSeconds(10), token);
            }
        }

        private async Task CheckPrintQueuesAsync(CancellationToken token)
        {
            if (!_isAutoPurgeEnabled())
            {
                return;
            }

            IReadOnlyList<JobSnapshot> jobs;
            try
            {
                jobs = await _probe.GetJobsAsync(_monitoredPrinters, token);
            }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR] GetJobs failed", ex);
                return;
            }

            var currentlySeen = new HashSet<int>();

            // Group by printer for cleaner log lines.
            var byPrinter = new Dictionary<string, List<JobSnapshot>>();
            foreach (var job in jobs)
            {
                if (!byPrinter.TryGetValue(job.PrinterName, out var list))
                {
                    list = new List<JobSnapshot>();
                    byPrinter[job.PrinterName] = list;
                }
                list.Add(job);
                currentlySeen.Add(job.JobId);
            }

            foreach (var (printer, list) in byPrinter)
            {
                foreach (var job in list)
                {
                    if (IsHardError(job.Status))
                    {
                        var streak = _hardErrorStreak.AddOrUpdate(job.JobId, 1, (_, v) => v + 1);

                        // Streak cap: re-arm if the job has been stuck too long without resolution.
                        if (streak > StreakCap)
                        {
                            _hardErrorStreak.TryRemove(job.JobId, out _);
                            _seenIncidents.TryRemove(job.JobId, out _);
                            continue;
                        }

                        if (streak < 2) continue; // wait for stable detection

                        if (_seenIncidents.TryAdd(job.JobId,
                                new IncidentRecord(printer, job.JobName, DateTime.UtcNow)))
                        {
                            AppLogger.Error(
                                $"[MONITOR] Auto-purging job {job.JobId} ({job.JobName}) on {printer} " +
                                $"(streak={streak}, status={job.Status}).");

                            try
                            {
                                await _probe.CancelAsync(job.JobId, printer, token);
                            }
                            catch (Exception ex)
                            {
                                AppLogger.Log(
                                    $"[MONITOR] Cancel for job {job.JobId} on {printer} failed: {ex.Message}. " +
                                    "Evicting dedup state.");
                                _hardErrorStreak.TryRemove(job.JobId, out _);
                                _seenIncidents.TryRemove(job.JobId, out _);
                                continue;
                            }

                            FirePurgeAlert(printer, job);
                        }
                    }
                    else
                    {
                        _hardErrorStreak.TryRemove(job.JobId, out _);
                    }
                }
            }

            // Eviction: drop dedup state for any jobId no longer present.
            foreach (var key in _seenIncidents.Keys)
            {
                if (!currentlySeen.Contains(key))
                    _seenIncidents.TryRemove(key, out _);
            }
            foreach (var key in _hardErrorStreak.Keys)
            {
                if (!currentlySeen.Contains(key))
                    _hardErrorStreak.TryRemove(key, out _);
            }
        }

        private void FirePurgeAlert(string printerName, JobSnapshot job)
        {
            var message = $"Auto-purged job {job.JobId} on '{printerName}': {job.Status}. " +
                          "Please check the printer physically (paper jam / out of paper).";

            try
            {
                _notificationService.ShowPrinterError(printerName, $"Job {job.JobId}: {job.Status}");
            }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR] Toast dispatch failed", ex);
            }

            // Surface the alert to the shell (replaces the WPF snackbar path).
            try
            {
                AlertRaised?.Invoke(new PrintMonitorAlert(printerName, job, message));
            }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR] Alert event dispatch failed", ex);
            }
        }
    }
}