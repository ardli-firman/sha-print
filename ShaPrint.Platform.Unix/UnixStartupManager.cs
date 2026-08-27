using System.Text;
using ShaPrint.Core;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Unix;

/// <summary>
/// <see cref="IStartupManager"/> for macOS/Linux.
///
/// macOS: a LaunchAgent plist at <c>~/Library/LaunchAgents/com.shaprint.app.plist</c>,
/// registered with <c>launchctl load/unload -w</c>.
/// Linux: a systemd user unit at <c>~/.config/systemd/user/shaprint.service</c>, enabled
/// with <c>systemctl --user daemon-reload/enable/disable</c>.
///
/// Both targets are user-writable, so no privilege escalation is needed (unlike the CUPS
/// component paths). Startup launches the running executable with <c>--startup</c>, mirroring
/// the Windows Task Scheduler task.
/// </summary>
public sealed class UnixStartupManager : IStartupManager
{
    private const string LaunchAgentLabel = "com.shaprint.app";
    private const string LaunchAgentFileName = "com.shaprint.app.plist";
    private const string SystemdServiceName = "shaprint.service";

    private static string LaunchAgentPath
        => Path.Combine(HomeDir(), "Library", "LaunchAgents", LaunchAgentFileName);

    private static string SystemdUserDir
        => Path.Combine(HomeDir(), ".config", "systemd", "user");

    private static string HomeDir()
        => Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

    public void SetStartup(bool enable)
    {
        UnixProcessRunner.EnsureUnix();

        try
        {
            if (OperatingSystem.IsMacOS())
            {
                SetMacOsStartup(enable);
            }
            else if (OperatingSystem.IsLinux())
            {
                SetLinuxStartup(enable);
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[STARTUP] Failed to change startup settings: " + ex.Message);
        }
    }

    public bool IsStartupEnabled()
    {
        UnixProcessRunner.EnsureUnix();

        try
        {
            if (OperatingSystem.IsMacOS())
            {
                // The plist file presence is the source of truth; launchctl load -w pins it.
                return File.Exists(LaunchAgentPath);
            }

            if (OperatingSystem.IsLinux())
            {
                var result = UnixProcessRunner.Run("systemctl", new[] { "--user", "is-enabled", SystemdServiceName });
                return result.Succeeded && result.StdOut.Trim() == "enabled";
            }

            return false;
        }
        catch (Exception ex)
        {
            AppLogger.Error("[STARTUP] Failed to check startup status: " + ex.Message);
            return false;
        }
    }

    // ── macOS ───────────────────────────────────────────────────────────────────

    private void SetMacOsStartup(bool enable)
    {
        string plistPath = LaunchAgentPath;
        if (enable)
        {
            string? exePath = Environment.ProcessPath;
            if (string.IsNullOrEmpty(exePath))
            {
                AppLogger.Error("[STARTUP] Cannot enable startup: current executable path is unknown.");
                return;
            }

            Directory.CreateDirectory(Path.GetDirectoryName(plistPath)!);
            File.WriteAllText(plistPath, BuildLaunchAgentPlist(exePath), new UTF8Encoding(false));

            var load = UnixProcessRunner.Run("launchctl", new[] { "load", "-w", plistPath });
            LogResult("launchctl load -w", load);
        }
        else
        {
            if (File.Exists(plistPath))
            {
                var unload = UnixProcessRunner.Run("launchctl", new[] { "unload", "-w", plistPath });
                LogResult("launchctl unload -w", unload);
                File.Delete(plistPath);
            }
            else
            {
                AppLogger.Log("[STARTUP] macOS LaunchAgent already absent — nothing to unload.");
            }
        }
    }

    internal static string BuildLaunchAgentPlist(string exePath)
    {
        // ProgramArguments is the only thing that needs escaping; XML-escape the path.
        string escaped = System.Security.SecurityElement.Escape(exePath);
        return $$"""
            <?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist version="1.0">
            <dict>
                <key>Label</key>
                <string>{{LaunchAgentLabel}}</string>
                <key>ProgramArguments</key>
                <array>
                    <string>{{escaped}}</string>
                    <string>--startup</string>
                </array>
                <key>RunAtLoad</key>
                <true/>
                <key>KeepAlive</key>
                <false/>
            </dict>
            </plist>
            """;
    }

    // ── Linux ───────────────────────────────────────────────────────────────────

    private void SetLinuxStartup(bool enable)
    {
        string servicePath = Path.Combine(SystemdUserDir, SystemdServiceName);

        if (enable)
        {
            string? exePath = Environment.ProcessPath;
            if (string.IsNullOrEmpty(exePath))
            {
                AppLogger.Error("[STARTUP] Cannot enable startup: current executable path is unknown.");
                return;
            }

            Directory.CreateDirectory(SystemdUserDir);
            File.WriteAllText(servicePath, BuildSystemdService(exePath), new UTF8Encoding(false));

            RunUserSystemd("daemon-reload");
            RunUserSystemd("enable", SystemdServiceName);
        }
        else
        {
            RunUserSystemd("disable", SystemdServiceName);
            RunUserSystemd("daemon-reload");
            if (File.Exists(servicePath))
            {
                File.Delete(servicePath);
            }
        }
    }

    internal static string BuildSystemdService(string exePath)
    {
        // systemd ExecStart quoting (NOT XML escaping — a .service file is not XML):
        // double-quote the path and escape embedded quotes/backslashes per systemd rules.
        string quoted = "\"" + exePath.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";
        return $$"""
            [Unit]
            Description=ShaPrint App
            After=network-online.target
            Wants=network-online.target

            [Service]
            Type=simple
            ExecStart={{quoted}} --startup
            Restart=on-failure

            [Install]
            WantedBy=default.target
            """;
    }

    private static void RunUserSystemd(params string[] args)
    {
        var result = UnixProcessRunner.Run("systemctl", new[] { "--user" }.Concat(args));
        LogResult($"systemctl --user {string.Join(' ', args)}", result);
    }

    private static void LogResult(string operation, ProcessResult result)
    {
        if (result.Succeeded)
        {
            AppLogger.Log($"[STARTUP] {operation} — ok.");
        }
        else
        {
            AppLogger.Error($"[STARTUP] {operation} failed (exit {result.ExitCode}): {result.StdErr.Trim()}");
        }
    }
}
