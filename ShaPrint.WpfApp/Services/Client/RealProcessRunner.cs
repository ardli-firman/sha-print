using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core.Abstractions;

namespace ShaPrint.WpfApp.Services.Client
{
    /// <summary>
    /// Production implementation of IProcessRunner using System.Diagnostics.Process.
    /// </summary>
    public class RealProcessRunner : IProcessRunner
    {
        public async Task<ProcessResult> RunAsync(string fileName, string arguments, TimeSpan? timeout = null)
            => await RunAsync(fileName, arguments, timeout, CancellationToken.None).ConfigureAwait(false);

        public async Task<ProcessResult> RunAsync(
            string fileName,
            string arguments,
            TimeSpan? timeout,
            CancellationToken cancellationToken)
        {
            var effectiveTimeout = timeout ?? TimeSpan.FromSeconds(30);
            if (effectiveTimeout <= TimeSpan.Zero)
                effectiveTimeout = TimeSpan.FromMilliseconds(1);

            var psi = new ProcessStartInfo
            {
                FileName = fileName,
                Arguments = arguments,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            try
            {
                using var process = new Process { StartInfo = psi, EnableRaisingEvents = true };
                if (!process.Start())
                {
                    return Failure("Process did not start.");
                }

                // Drain both pipes immediately and concurrently. Waiting for one
                // pipe before starting the other can deadlock a noisy child.
                Task<string> stdoutTask = ReadBoundedAsync(process.StandardOutput, 1_048_576);
                Task<string> stderrTask = ReadBoundedAsync(process.StandardError, 1_048_576);
                using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
                timeoutCts.CancelAfter(effectiveTimeout);

                bool timedOut = false;
                bool canceled = false;
                try
                {
                    await process.WaitForExitAsync(timeoutCts.Token).ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
                {
                    timedOut = true;
                    KillProcessTree(process);
                }
                catch (OperationCanceledException)
                {
                    canceled = true;
                    KillProcessTree(process);
                }

                // KillProcessTree closes the child's handles, allowing both drain
                // tasks to finish. Observe both tasks even on failure.
                string stdout = await ObserveOutputAsync(stdoutTask).ConfigureAwait(false);
                string stderr = await ObserveOutputAsync(stderrTask).ConfigureAwait(false);

                if (timedOut)
                    return Failure("Process timed out.", stdout, stderr);
                if (canceled)
                    return Failure("Process cancelled.", stdout, stderr);

                return new ProcessResult
                {
                    ExitCode = process.ExitCode,
                    Output = stdout.Trim(),
                    Error = stderr.Trim()
                };
            }
            catch (Exception ex)
            {
                return Failure(ex.Message);
            }
        }

        private static ProcessResult Failure(string message, string output = "", string error = "")
            => new()
            {
                ExitCode = -1,
                Output = output.Trim(),
                Error = string.IsNullOrWhiteSpace(error) ? message : $"{message} {error.Trim()}"
            };

        private static async Task<string> ObserveOutputAsync(Task<string> task)
        {
            try
            {
                Task completed = await Task.WhenAny(task, Task.Delay(TimeSpan.FromSeconds(2))).ConfigureAwait(false);
                if (completed != task)
                {
                    _ = task.ContinueWith(
                        observed => _ = observed.Exception,
                        CancellationToken.None,
                        TaskContinuationOptions.OnlyOnFaulted | TaskContinuationOptions.ExecuteSynchronously,
                        TaskScheduler.Default);
                    return "[process output drain timed out]";
                }
                return await task.ConfigureAwait(false);
            }
            catch (Exception ex) { return $"[output unavailable: {ex.Message}]"; }
        }

        private static async Task<string> ReadBoundedAsync(StreamReader reader, int maxChars)
        {
            var builder = new StringBuilder(Math.Min(maxChars, 16 * 1024));
            char[] buffer = new char[4096];
            int total = 0;
            while (true)
            {
                int read = await reader.ReadAsync(buffer.AsMemory()).ConfigureAwait(false);
                if (read == 0) break;
                int keep = Math.Min(read, maxChars - total);
                if (keep > 0)
                    builder.Append(buffer, 0, keep);
                total += read;
                if (total >= maxChars)
                {
                    builder.Append("\n[process output truncated]");
                    // Continue draining without retaining unbounded output.
                    while (await reader.ReadAsync(buffer.AsMemory()).ConfigureAwait(false) > 0) { }
                    break;
                }
            }
            return builder.ToString();
        }

        private static void KillProcessTree(Process process)
        {
            try
            {
                if (!process.HasExited)
                    process.Kill(entireProcessTree: true);
            }
            catch { /* process may have exited between the checks */ }
        }
    }
}
