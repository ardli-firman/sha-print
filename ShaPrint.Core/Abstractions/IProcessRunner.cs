using System;
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
    }
}
