using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Printing;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using Wpf.Ui;
using ShaPrint.WpfApp.Models;
using ShaPrint.WpfApp.Services;

namespace ShaPrint.WpfApp.Services.Server
{
    public class PrintMonitorService
    {
        private const int StreakCap = 10;

        private CancellationTokenSource? _cts;
        private readonly ISnackbarService _snackbarService;
        private readonly INotificationService _notificationService;
        private readonly IPrintQueueProbe _probe;
        private readonly IDelayProbe _delay;
        private List<string> _monitoredPrinters = new List<string>();

        // Per-jobId: how many consecutive polls it has been in a hard error state.
        private readonly ConcurrentDictionary<int, int> _hardErrorStreak = new();

        // Per-jobId: the set of incidents we have already cancelled + alerted on.
        private readonly ConcurrentDictionary<int, IncidentRecord> _seenIncidents = new();

        public PrintMonitorService(
            ISnackbarService snackbarService,
            INotificationService notificationService,
            IPrintQueueProbe probe,
            IDelayProbe delay)
        {
            _snackbarService = snackbarService;
            _notificationService = notificationService;
            _probe = probe;
            _delay = delay;
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

        public static bool IsHardError(PrintJobStatus status)
        {
            return status.HasFlag(PrintJobStatus.Error) ||
                   status.HasFlag(PrintJobStatus.PaperOut) ||
                   status.HasFlag(PrintJobStatus.Blocked);
        }

        private async Task MonitorLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    if (AppSettings.Current.AutoPurgeEnabled)
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
            if (!AppSettings.Current.AutoPurgeEnabled)
            {
                return;
            }

            IReadOnlyList<JobSnapshot> jobs;
            try
            {
                jobs = await _probe.GetJobsAsync(_monitoredPrinters);
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
                                await _probe.CancelAsync(job.JobId, printer);
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
                var dispatcher = System.Windows.Application.Current?.Dispatcher;
                if (dispatcher != null)
                {
                    dispatcher.InvokeAsync(() =>
                    {
                        _snackbarService.Show(
                            "Print Job Failed",
                            message,
                            Wpf.Ui.Controls.ControlAppearance.Danger,
                            new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.ErrorCircle24),
                            TimeSpan.FromSeconds(10));
                    });
                }
                else
                {
                    _snackbarService.Show(
                        "Print Job Failed",
                        message,
                        Wpf.Ui.Controls.ControlAppearance.Danger,
                        null,
                        TimeSpan.FromSeconds(10));
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR] Snackbar dispatch failed", ex);
            }

            try
            {
                _notificationService.ShowPrinterError(printerName, $"Job {job.JobId}: {job.Status}");
            }
            catch (Exception ex)
            {
                AppLogger.Error("[MONITOR] Toast dispatch failed", ex);
            }
        }
    }
}
