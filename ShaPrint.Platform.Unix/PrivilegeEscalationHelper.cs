using System.Text;
using ShaPrint.Core;

namespace ShaPrint.Platform.Unix;

/// <summary>
/// Detects permission failures when writing to root-owned system paths
/// (<c>/usr/lib/cups/backend</c>, <c>/usr/share/cups/model</c>) and returns actionable
/// <c>sudo</c>/<c>pkexec</c> instructions instead of pretending the write succeeded.
///
/// The Unix backends NEVER assume <c>File.WriteAllText</c> to a system path works: every
/// such write goes through <see cref="WriteSystemFile"/>, which attempts the write and, on
/// <see cref="UnauthorizedAccessException"/>/<see cref="IOException"/>, stages the content
/// in a user-writable temp file and reports the exact command the user must run (or the
/// packaging <c>postinst</c> hook must perform). There is deliberately no "fake success" —
/// the caller only continues when <c>Success == true</c>.
/// </summary>
public static class PrivilegeEscalationHelper
{
    private static bool? _isRoot;

    /// <summary>
    /// True when the current process runs as uid 0 (root), checked via <c>id -u</c> and
    /// cached for the process lifetime.
    /// </summary>
    public static bool IsRunningAsRoot()
    {
        if (_isRoot.HasValue)
        {
            return _isRoot.Value;
        }

        try
        {
            var result = UnixProcessRunner.Run("id", new[] { "-u" });
            _isRoot = result.Succeeded && result.StdOut.Trim() == "0";
        }
        catch
        {
            _isRoot = false;
        }
        return _isRoot.Value;
    }

    /// <summary>
    /// Writes <paramref name="content"/> to <paramref name="destPath"/> (typically under a
    /// root-owned directory). On success returns <c>(true, "")</c>; on a permission failure
    /// returns <c>(false, ...)</c> where the message contains the exact command the user can
    /// run with <c>sudo</c>/<c>pkexec</c> to complete the install.
    /// </summary>
    public static (bool Success, string ErrorMessage) WriteSystemFile(
        string destPath,
        string content,
        bool executable)
    {
        string? dir = Path.GetDirectoryName(destPath);
        if (string.IsNullOrEmpty(dir) || !Directory.Exists(dir))
        {
            return (false,
                $"Target directory '{dir}' does not exist — is CUPS installed?\n" +
                "Install CUPS first (macOS: built-in; Linux: `sudo apt install cups` or distro equivalent).");
        }

        try
        {
            File.WriteAllText(destPath, content, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false));
            if (executable)
            {
                ApplyExecutableBit(destPath);
            }
            AppLogger.Log($"[PRIV] Wrote {destPath} (executable={executable}).");
            return (true, string.Empty);
        }
        catch (UnauthorizedAccessException ex)
        {
            string instruction = BuildInstallInstruction(destPath, content, executable);
            AppLogger.Error($"[PRIV] Permission denied writing {destPath}: {ex.Message}\n{instruction}");
            return (false, instruction);
        }
        catch (IOException ex) when (IsPermissionFailure(ex))
        {
            string instruction = BuildInstallInstruction(destPath, content, executable);
            AppLogger.Error($"[PRIV] Could not write {destPath}: {ex.Message}\n{instruction}");
            return (false, instruction);
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[PRIV] Unexpected error writing {destPath}: {ex}");
            return (false, $"Unexpected error writing {destPath}: {ex.Message}");
        }
    }

    /// <summary>
    /// Produces a <c>sudo</c>/<c>pkexec</c> instruction for a command that needs root
    /// (e.g. an <c>lpadmin</c> invocation that was rejected for lack of permission).
    /// </summary>
    public static string ElevationInstruction(string command)
        => $"Run one of these in a terminal:\n" +
           $"  sudo {command}\n" +
           $"  # or: pkexec {command}\n" +
           "(or let the package postinst hook perform this step during installation).";

    private static void ApplyExecutableBit(string path)
    {
        // File.SetUnixFileMode is Unix-only; this helper is only reached through the Unix
        // runtime guard, and the explicit check also silences CA1416 on Windows builds.
        if (!OperatingSystem.IsMacOS() && !OperatingSystem.IsLinux())
        {
            return;
        }

        const UnixFileMode mode =
            UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute |
            UnixFileMode.GroupRead | UnixFileMode.GroupExecute |
            UnixFileMode.OtherRead | UnixFileMode.OtherExecute; // 0755
        File.SetUnixFileMode(path, mode);
    }

    private static bool IsPermissionFailure(IOException ex)
    {
        // errno EACCES (13) / EROFS (30) surface on Unix as IOException/UnauthorizedAccessException.
        return ex.HResult is 13 or 30 || ex.InnerException is UnauthorizedAccessException;
    }

    private static string BuildInstallInstruction(string destPath, string content, bool executable)
    {
        // Stage the content where the user can reach it with sudo, then hand over the exact
        // command. A deterministic temp name keeps failed attempts from accumulating files.
        string tmp = Path.Combine(Path.GetTempPath(), $"shaprint_install_{Path.GetFileName(destPath)}");
        try
        {
            File.WriteAllText(tmp, content, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false));
        }
        catch (Exception ex)
        {
            return $"Could not write {destPath} and could not stage a sudo helper file: {ex.Message}";
        }

        string mode = executable ? "755" : "644";
        return
            $"ShaPrint needs root to install a CUPS component.\n" +
            $"The content is staged at: {tmp}\n" +
            $"Run one of these in a terminal:\n" +
            $"  sudo install -m {mode} '{tmp}' '{destPath}'\n" +
            $"  # or: pkexec install -m {mode} '{tmp}' '{destPath}'\n" +
            "(when this app is packaged, the package postinst hook performs this step automatically).";
    }
}
