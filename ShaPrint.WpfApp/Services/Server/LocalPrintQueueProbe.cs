using System;
using System.Collections.Generic;
using System.Linq;
using System.Printing;
using System.Threading.Tasks;
using ShaPrint.Core;

namespace ShaPrint.WpfApp.Services.Server
{
    /// <summary>
    /// Real IPrintQueueProbe backed by Windows' LocalPrintServer.
    /// </summary>
    public sealed class LocalPrintQueueProbe : IPrintQueueProbe
    {
        public Task<IReadOnlyList<JobSnapshot>> GetJobsAsync(IEnumerable<string> monitoredPrinters)
        {
            return Task.Run<IReadOnlyList<JobSnapshot>>(() =>
            {
                var result = new List<JobSnapshot>();
                using var server = new LocalPrintServer();
                var names = monitoredPrinters as ICollection<string> ?? monitoredPrinters.ToList();

                foreach (var name in names)
                {
                    PrintQueue? queue = null;
                    try
                    {
                        queue = server.GetPrintQueue(name);
                        queue.Refresh();
                        foreach (var job in queue.GetPrintJobInfoCollection())
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
            });
        }

        public async Task CancelAsync(int jobId, string printerName)
        {
            await Task.Run(() =>
            {
                using var server = new LocalPrintServer();
                using var queue = server.GetPrintQueue(printerName);
                foreach (var job in queue.GetPrintJobInfoCollection())
                {
                    if (job.JobIdentifier == jobId)
                    {
                        job.Cancel();
                        return;
                    }
                }
                throw new InvalidOperationException(
                    $"Job {jobId} no longer exists on printer '{printerName}'.");
            });
        }
    }
}
