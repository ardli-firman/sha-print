using System.Diagnostics;
using ShaPrint.Core;

namespace ShaPrint.Platform.Unix;

/// <summary>
/// Result of a CLI invocation with stdout/stderr captured (no shell involved).
/// </summary>
internal sealed record ProcessResult(int ExitCode, string StdOut, string StdErr)
{
    public bool Succeeded => ExitCode == 0;
}

/// <summary>
/// Minimal CLI-first process runner shared by every Unix backend.
///
/// Every invocation uses <c>ProcessStartInfo.ArgumentList</c> with
/// <c>UseShellExecute=false</c> and redirected streams, so shell metacharacters
/// (<c>&gt;</c>, <c>|</c>, quotes) are NEVER interpreted: printer/scanner/file names
/// cannot inject commands, and a literal <c>&gt;</c> cannot be (ab)used as a redirect.
///
/// The same "no P/Invoke, no libcups/libsane" rule that drives the backends applies here —
/// launching a CLI process is the only OS integration this project needs.
/// </summary>
internal static class UnixProcessRunner
{
    /// <summary>
    /// Runtime guard: the Unix backends only run on macOS/Linux. Throws a clear
    /// <see cref="PlatformNotSupportedException"/> otherwise instead of misbehaving.
    /// </summary>
    public static void EnsureUnix()
    {
        if (!OperatingSystem.IsMacOS() && !OperatingSystem.IsLinux())
        {
            throw new PlatformNotSupportedException(
                "ShaPrint.Platform.Unix backends only run on macOS or Linux. " +
                $"Detected OS: {Environment.OSVersion}.");
        }
    }

    /// <summary>
    /// True when <paramref name="command"/> is available on PATH. Uses the POSIX
    /// <c>command -v</c> builtin (more portable than <c>which</c>, which is not
    /// guaranteed to be installed on every macOS/Linux box).
    /// </summary>
    public static bool CommandExists(string command)
    {
        try
        {
            var result = Run("/bin/sh", new[] { "-c", $"command -v {command}" });
            return result.Succeeded && !string.IsNullOrWhiteSpace(result.StdOut);
        }
        catch
        {
            return false;
        }
    }

    /// <summary>
    /// Runs <paramref name="fileName"/> with <paramref name="arguments"/> and returns its
    /// exit code plus captured stdout/stderr. Never throws for a non-zero exit code or a
    /// missing executable — those are returned as a failed <see cref="ProcessResult"/>.
    /// </summary>
    public static ProcessResult Run(
        string fileName,
        IEnumerable<string>? arguments = null,
        TimeSpan? timeout = null)
    {
        var psi = new ProcessStartInfo
        {
            FileName = fileName,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            RedirectStandardInput = true,
            CreateNoWindow = true,
        };
        if (arguments != null)
        {
            foreach (var arg in arguments)
            {
                psi.ArgumentList.Add(arg);
            }
        }

        using var process = new Process { StartInfo = psi };
        if (!Start(process, fileName, out var startError))
        {
            return new ProcessResult(-1, string.Empty, startError);
        }

        // Async reads must be running before WaitForExit, or a chatty child (lpinfo -m,
        // scanimage -L) can fill the pipe buffer and deadlock.
        var stdoutTask = process.StandardOutput.ReadToEndAsync();
        var stderrTask = process.StandardError.ReadToEndAsync();

        var effectiveTimeout = timeout ?? TimeSpan.FromSeconds(60);
        if (!process.WaitForExit((int)effectiveTimeout.TotalMilliseconds))
        {
            try { process.Kill(entireProcessTree: true); } catch { /* already exited */ }
            AppLogger.Error($"[PROC] {fileName} timed out after {effectiveTimeout.TotalSeconds}s.");
            return new ProcessResult(-1, string.Empty, $"Process timed out after {effectiveTimeout.TotalSeconds}s.");
        }

        Task.WaitAll(stdoutTask, stderrTask);
        return new ProcessResult(process.ExitCode, stdoutTask.Result, stderrTask.Result);
    }

    /// <summary>
    /// Runs <paramref name="fileName"/> whose STDOUT is binary data (scanimage image
    /// output) and returns the raw bytes. The <c>&gt;</c> shell redirect is NOT used —
    /// bytes are copied from the redirected output stream by this process.
    /// </summary>
    public static (byte[] Output, string StdErr, int ExitCode) RunBinaryOutput(
        string fileName,
        IEnumerable<string> arguments,
        TimeSpan? timeout = null)
    {
        var psi = new ProcessStartInfo
        {
            FileName = fileName,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            RedirectStandardInput = true,
            CreateNoWindow = true,
        };
        foreach (var arg in arguments)
        {
            psi.ArgumentList.Add(arg);
        }

        using var process = new Process { StartInfo = psi };
        if (!Start(process, fileName, out var startError))
        {
            return (Array.Empty<byte>(), startError, -1);
        }

        var stderrTask = process.StandardError.ReadToEndAsync();
        using var outputStream = new MemoryStream();
        var copyTask = process.StandardOutput.BaseStream.CopyToAsync(outputStream);

        var effectiveTimeout = timeout ?? TimeSpan.FromSeconds(120);
        if (!process.WaitForExit((int)effectiveTimeout.TotalMilliseconds))
        {
            try { process.Kill(entireProcessTree: true); } catch { /* already exited */ }
            AppLogger.Error($"[PROC] {fileName} timed out after {effectiveTimeout.TotalSeconds}s.");
            return (Array.Empty<byte>(), $"Process timed out after {effectiveTimeout.TotalSeconds}s.", -1);
        }

        copyTask.GetAwaiter().GetResult();
        return (outputStream.ToArray(), stderrTask.Result, process.ExitCode);
    }

    private static bool Start(Process process, string fileName, out string error)
    {
        try
        {
            if (!process.Start())
            {
                error = "Failed to start process.";
                AppLogger.Error($"[PROC] Failed to start {fileName}: {error}");
                return false;
            }
        }
        catch (Exception ex)
        {
            // Missing executable (e.g. CUPS/SANE tool not installed) surfaces here.
            error = ex.Message;
            AppLogger.Error($"[PROC] Failed to start {fileName}: {ex.Message}");
            return false;
        }
        error = string.Empty;
        return true;
    }
}
