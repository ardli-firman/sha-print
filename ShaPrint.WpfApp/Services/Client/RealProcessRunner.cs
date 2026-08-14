using System;
using System.Diagnostics;
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
        {
            var tcs = new TaskCompletionSource<ProcessResult>();
            var effectiveTimeout = timeout ?? TimeSpan.FromSeconds(30);

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
                using var process = new Process { StartInfo = psi };
                process.Start();

                string stdout = await process.StandardOutput.ReadToEndAsync();
                string stderr = await process.StandardError.ReadToEndAsync();

                bool exited = await Task.Run(() => process.WaitForExit((int)effectiveTimeout.TotalMilliseconds));

                return new ProcessResult
                {
                    ExitCode = exited ? process.ExitCode : -1,
                    Output = stdout.Trim(),
                    Error = stderr.Trim()
                };
            }
            catch (Exception ex)
            {
                return new ProcessResult
                {
                    ExitCode = -1,
                    Output = string.Empty,
                    Error = ex.Message
                };
            }
        }
    }
}
