#if WINDOWS
using System;
using System.Collections.Generic;
using System.Linq;
using System.Printing;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;

namespace ShaPrint.UI.Services
{
    /// <summary>
    /// Real <see cref="IPrintQueueProbe"/> backed by Windows' <c>LocalPrintServer</c>.
    /// Compiled only for the net8.0-windows TFM (#if WINDOWS) because <c>System.Printing</c>
    /// lives in the Windows Desktop framework — the plain net8.0 build (macOS/Linux) must not
    /// reference it. Kept in ShaPrint.UI (not ShaPrint.Platform.Windows) so it can implement
    /// the <see cref="IPrintQueueProbe"/> interface defined here without a circular project
    /// reference; the mirrored <c>PrintMonitorService</c> stays runnable on net8.0 because it
    /// only ever sees <see cref="MonitorJobStatus"/> / <see cref="JobSnapshot"/>.
    /// </summary>
    public sealed class LocalPrintQueueProbe : IPrintQueueProbe
    {
        public Task<IReadOnlyList<JobSnapshot>> GetJobsAsync(IEnumerable<string> monitoredPrinters, CancellationToken cancellationToken)
        {
            return Task.Run<IReadOnlyList<JobSnapshot>>(() =>
            {
                var result = new List<JobSnapshot>();
                using var server = new LocalPrintServer();
                var names = monitoredPrinters as ICollection<string> ?? monitoredPrinters.ToList();

                foreach (var name in names)
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    PrintQueue? queue = null;
                    try
                    {
                        queue = server.GetPrintQueue(name);
                        if (queue != null)
                        {
                            queue.Refresh();
                            using var jobs = queue.GetPrintJobInfoCollection();
                            foreach (var job in jobs)
                            {
                                using (job)
                                {
                                    try
                                    {
                                        result.Add(new JobSnapshot(
                                            JobId: job.JobIdentifier,
                                            PrinterName: queue.Name,
                                            JobName: job.Name,
                                            Status: FromSystem(job.JobStatus)));
                                    }
                                    catch (Exception ex)
                                    {
                                        AppLogger.Log($"[PROBE] Failed to read job in queue '{name}': {ex.Message}");
                                    }
                                }
                            }
                        }
                    }
                    catch (Exception ex)
                    {
                        AppLogger.Log($"[PROBE] Failed to read queue '{name}': {ex.Message}");
                    }
                    finally
                    {
                        queue?.Dispose();
                    }
                }
                return result;
            }, cancellationToken);
        }

        public async Task CancelAsync(int jobId, string printerName, CancellationToken cancellationToken)
        {
            await Task.Run(() =>
            {
                using var server = new LocalPrintServer();
                using var queue = server.GetPrintQueue(printerName)
                    ?? throw new InvalidOperationException($"Printer '{printerName}' not found.");

                using var jobs = queue.GetPrintJobInfoCollection();
                foreach (var job in jobs)
                {
                    using (job)
                    {
                        if (job.JobIdentifier == jobId)
                        {
                            job.Cancel();
                            return;
                        }
                    }
                }
                throw new InvalidOperationException(
                    $"Job {jobId} no longer exists on printer '{printerName}'.");
            }, cancellationToken);
        }

        /// <summary>
        /// Maps <c>System.Printing.PrintJobStatus</c> onto <see cref="MonitorJobStatus"/>. The
        /// hard-error bits the monitor acts on (Error/PaperOut/Blocked) are copied 1:1 so
        /// auto-purge behavior is byte-identical to the pre-migration WpfApp monitor.
        /// </summary>
        private static MonitorJobStatus FromSystem(PrintJobStatus status)
        {
            var result = MonitorJobStatus.None;
            if (status.HasFlag(PrintJobStatus.Error)) result |= MonitorJobStatus.Error;
            if (status.HasFlag(PrintJobStatus.PaperOut)) result |= MonitorJobStatus.PaperOut;
            if (status.HasFlag(PrintJobStatus.Blocked)) result |= MonitorJobStatus.Blocked;
            if (status.HasFlag(PrintJobStatus.Paused)) result |= MonitorJobStatus.Paused;
            if (status.HasFlag(PrintJobStatus.Printing)) result |= MonitorJobStatus.Printing;
            if (status.HasFlag(PrintJobStatus.Offline)) result |= MonitorJobStatus.Offline;
            if (status.HasFlag(PrintJobStatus.Printed)) result |= MonitorJobStatus.Printed;
            if (status.HasFlag(PrintJobStatus.Deleted)) result |= MonitorJobStatus.Deleted;
            if (status.HasFlag(PrintJobStatus.UserIntervention)) result |= MonitorJobStatus.UserIntervention;
            if (status.HasFlag(PrintJobStatus.Spooling)) result |= MonitorJobStatus.Spooling;
            if (status.HasFlag(PrintJobStatus.Retained)) result |= MonitorJobStatus.Retained;
            return result;
        }
    }
}
#endif