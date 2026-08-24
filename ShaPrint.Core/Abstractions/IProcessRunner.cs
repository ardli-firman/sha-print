using System;
using System.Threading;
using System.Threading.Tasks;

namespace ShaPrint.Core.Abstractions
{
    /// <summary>
    /// Result of an external process invocation.
    /// </summary>
    public class ProcessResult
    {
        public int ExitCode { get; set; }
        public string Output { get; set; } = string.Empty;
        public string Error { get; set; } = string.Empty;
        public bool Success => ExitCode == 0;
    }

    /// <summary>
    /// Abstraction over external process execution (powershell, pnputil, etc.).
    /// Enables unit testing without spawning real OS processes.
    /// </summary>
    public interface IProcessRunner
    {
        Task<ProcessResult> RunAsync(string fileName, string arguments, TimeSpan? timeout = null);

        /// <summary>
        /// Cancellation-aware process execution. The default implementation keeps
        /// existing test doubles/source compatibility while production runners may
        /// provide true process cancellation.
        /// </summary>
        Task<ProcessResult> RunAsync(
            string fileName,
            string arguments,
            TimeSpan? timeout,
            CancellationToken cancellationToken)
            => RunAsync(fileName, arguments, timeout);
    }
}
