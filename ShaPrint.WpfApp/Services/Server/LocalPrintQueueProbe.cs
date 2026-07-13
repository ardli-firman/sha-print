using System;
using System.Collections.Generic;
using System.Linq;
using System.Printing;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;

namespace ShaPrint.WpfApp.Services.Server
{
    /// <summary>
    /// Real IPrintQueueProbe backed by Windows' LocalPrintServer.
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
                                            Status: job.JobStatus));
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
    }
}
