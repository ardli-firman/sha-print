using System;
using System.Collections.Generic;
using System.Printing;
using System.Threading.Tasks;

namespace ShaPrint.WpfApp.Services.Server
{
    /// <summary>
    /// Snapshot of a single print job returned by the probe. Carries only
    /// the data the monitor needs to make decisions — no live handles.
    /// </summary>
    public record JobSnapshot(int JobId, string PrinterName, string JobName, PrintJobStatus Status);

    /// <summary>
    /// Record kept in the dedup dictionary for every jobId the monitor has
    /// already cancelled + alerted on. Used to suppress repeat alerts and
    /// to feed the log path.
    /// </summary>
    public record IncidentRecord(string PrinterName, string JobName, DateTime FirstSeenUtc);

    /// <summary>
    /// Abstraction over Windows Print Spooler queries. The real implementation
    /// wraps LocalPrintServer; tests provide a fake that returns scripted
    /// snapshots and records Cancel calls.
    /// </summary>
    public interface IPrintQueueProbe
    {
        /// <summary>
        /// Returns a snapshot of every job currently sitting in the given
        /// monitored printers. Implementations must NOT filter by error state.
        /// </summary>
        Task<IReadOnlyList<JobSnapshot>> GetJobsAsync(IEnumerable<string> monitoredPrinters);

        /// <summary>
        /// Cancels the job with the given id on the given printer. Throws if
        /// the queue or job is already gone — the caller treats that as
        /// "job has been cleaned up" and evicts dedup state.
        /// </summary>
        Task CancelAsync(int jobId, string printerName);
    }
}
