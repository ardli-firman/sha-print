using System;
using System.Printing;
using System.Threading;
using System.Threading.Tasks;
using System.Collections.Generic;
using ShaPrint.Core;
using Wpf.Ui;
using ShaPrint.WpfApp.Models;
using ShaPrint.WpfApp.Services;

namespace ShaPrint.WpfApp.Services.Server
{
    public class PrintMonitorService
    {
        private CancellationTokenSource? _cts;
        private readonly ISnackbarService _snackbarService;
        private readonly INotificationService _notificationService;
        private List<string> _monitoredPrinters = new List<string>();

        public PrintMonitorService(ISnackbarService snackbarService, INotificationService notificationService)
        {
            _snackbarService = snackbarService;
            _notificationService = notificationService;
        }

        public void SetMonitoredPrinters(List<string> printers)
        {
            _monitoredPrinters = printers ?? new List<string>();
        }

        public void Start()
        {
            if (_cts != null) return;
            _cts = new CancellationTokenSource();
            Task.Run(() => MonitorLoopAsync(_cts.Token));
        }

        public void Stop()
        {
            if (_cts == null) return;
            _cts.Cancel();
            _cts = null;
        }

        private async Task MonitorLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    if (AppSettings.Current.AutoPurgeEnabled)
                    {
                        CheckPrintQueues();
                    }
                }
                catch (Exception ex)
                {
                    AppLogger.Error("[MONITOR] Failed to check print queues", ex);
                }

                await Task.Delay(5000, token); // poll every 5 seconds
            }
        }

        private void CheckPrintQueues()
        {
            using var server = new LocalPrintServer();
            var queues = server.GetPrintQueues();

            foreach (var queue in queues)
            {
                if (!_monitoredPrinters.Contains(queue.Name))
                    continue;

                queue.Refresh();
                
                if (queue.NumberOfJobs > 0)
                {
                    var jobs = queue.GetPrintJobInfoCollection();
                    foreach (var job in jobs)
                    {
                        if (IsJobInErrorState(job.JobStatus))
                        {
                            AppLogger.Error($"[MONITOR] Auto-purging job {job.JobIdentifier} ({job.Name}) on {queue.Name} due to status {job.JobStatus}.");
                            
                            try
                            {
                                job.Cancel();
                                ShowAlert(queue.Name, job.Name);
                            }
                            catch (Exception ex)
                            {
                                AppLogger.Error($"[MONITOR] Failed to cancel job {job.JobIdentifier}.", ex);
                            }
                            _notificationService.ShowPrinterError(queue.Name,
                                $"Auto-purged job {job.JobIdentifier}: {job.JobStatus}");
                        }
                    }
                }
            }
        }

        public static bool IsJobInErrorState(PrintJobStatus status)
        {
            return status.HasFlag(PrintJobStatus.Error) || 
                   status.HasFlag(PrintJobStatus.PaperOut) || 
                   status.HasFlag(PrintJobStatus.Blocked) || 
                   status.HasFlag(PrintJobStatus.Offline);
        }

        private void ShowAlert(string printerName, string jobName)
        {
            System.Windows.Application.Current.Dispatcher.InvokeAsync(() =>
            {
                _snackbarService.Show(
                    "Print Job Failed",
                    $"Printing failed on '{printerName}'. The job was automatically cancelled. Please check the printer physically (paper jam / out of paper).",
                    Wpf.Ui.Controls.ControlAppearance.Danger,
                    new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.ErrorCircle24),
                    TimeSpan.FromSeconds(10)
                );
            });
        }
    }
}
