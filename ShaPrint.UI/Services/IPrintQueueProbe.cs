using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace ShaPrint.UI.Services
{
    /// <summary>
    /// Platform-agnostic snapshot of a print job's status bits. Mirrors the subset of
    /// <c>System.Printing.PrintJobStatus</c> that the monitor actually reacts to (hard-error
    /// flags Error/PaperOut/Blocked decide auto-purge) plus the common soft states used for
    /// logging. Bit values deliberately match <c>PrintJobStatus</c> where the names do.
    /// The Windows probe (<see cref="LocalPrintQueueProbe"/>, #if WINDOWS) converts from
    /// <c>PrintJobStatus</c>; non-Windows hosts provide their own probe or none at all.
    /// </summary>
    [Flags]
    public enum MonitorJobStatus
    {
        None = 0,
        Paused = 1,
        Error = 2,
        Printing = 4,
        Offline = 8,
        PaperOut = 16,
        Printed = 32,
        Deleted = 64,
        Blocked = 128,
        UserIntervention = 256,
        Spooling = 512,
        Retained = 4096,
    }

    /// <summary>
    /// Snapshot of a single print job returned by the probe. Carries only
    /// the data the monitor needs to make decisions — no live handles.
    /// </summary>
    public record JobSnapshot(int JobId, string PrinterName, string JobName, MonitorJobStatus Status);

    /// <summary>
    /// Abstraction over print-spooler queue queries. The Windows implementation wraps
    /// <c>LocalPrintServer</c> (<see cref="LocalPrintQueueProbe"/>); tests provide a fake that
    /// returns scripted snapshots and records Cancel calls.
    /// </summary>
    public interface IPrintQueueProbe
    {
        /// <summary>
        /// Returns a snapshot of every job currently sitting in the given
        /// monitored printers. Implementations must NOT filter by error state.
        /// </summary>
        Task<IReadOnlyList<JobSnapshot>> GetJobsAsync(IEnumerable<string> monitoredPrinters, CancellationToken cancellationToken);

        /// <summary>
        /// Cancels the job with the given id on the given printer. Throws if
        /// the queue or job is already gone — the caller treats that as
        /// "job has been cleaned up" and evicts dedup state.
        /// </summary>
        Task CancelAsync(int jobId, string printerName, CancellationToken cancellationToken);
    }
}