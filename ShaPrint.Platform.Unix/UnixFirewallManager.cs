using ShaPrint.Core;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Unix;

/// <summary>
/// <see cref="IFirewallManager"/> for macOS/Linux — best-effort only. Firewall changes
/// require root, and this manager must NEVER crash or block the app when permissions are
/// missing: failed attempts are logged together with the exact command the user can run.
///
/// macOS: guidance for the System Settings firewall UI; when running as root, the app is
/// registered with <c>socketfilterfw</c> (the correct mechanism to allow an app through the
/// application firewall — <c>pfctl</c> anchor management is deliberately out of scope for
/// v1; blindly enabling <c>pfctl</c> would alter the system packet filter).
/// Linux: detects <c>ufw</c> → <c>firewall-cmd</c> → <c>iptables</c> and opens
/// UDP 9876 (discovery), TCP 9877 (print relay), TCP 9878 (monitor), via <c>sudo -n</c>
/// (fails fast instead of blocking on a password prompt).
/// </summary>
public sealed class UnixFirewallManager : IFirewallManager
{
    private static readonly string[] Ports = { "9876/udp", "9877/tcp", "9878/tcp" };

    public async Task EnsureFirewallRulesAsync()
    {
        UnixProcessRunner.EnsureUnix();

        try
        {
            if (OperatingSystem.IsMacOS())
            {
                EnsureMacOs();
            }
            else if (OperatingSystem.IsLinux())
            {
                await EnsureLinuxAsync();
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[FIREWALL] Unexpected error: " + ex.Message);
        }
    }

    // ── macOS ───────────────────────────────────────────────────────────────────

    private void EnsureMacOs()
    {
        if (PrivilegeEscalationHelper.IsRunningAsRoot() && Environment.ProcessPath is { Length: > 0 } exe)
        {
            var add = UnixProcessRunner.Run("/usr/libexec/ApplicationFirewall/socketfilterfw", new[] { "--add", exe });
            var unblock = UnixProcessRunner.Run("/usr/libexec/ApplicationFirewall/socketfilterfw", new[] { "--unblockapp", exe });
            AppLogger.Log($"[FIREWALL] socketfilterfw add={add.Succeeded} unblock={unblock.Succeeded} for '{exe}'.");
            return;
        }

        AppLogger.Log(
            "[FIREWALL] macOS firewall rules need root. Add ShaPrint to the allowed apps manually: " +
            "System Settings → Network → Firewall → Options → allow ShaPrint for " +
            $"TCP {Ports[1].Split('/')[0]}/{Ports[2].Split('/')[0]} and UDP {Ports[0].Split('/')[0]}, or run as root: " +
            "sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add /path/to/ShaPrint");
    }

    // ── Linux ───────────────────────────────────────────────────────────────────

    private async Task EnsureLinuxAsync()
    {
        if (UnixProcessRunner.CommandExists("ufw"))
        {
            foreach (var port in Ports)
            {
                await RunFirewallCommandAsync("ufw", new[] { "allow", port });
            }
            return;
        }

        if (UnixProcessRunner.CommandExists("firewall-cmd"))
        {
            foreach (var port in Ports)
            {
                await RunFirewallCommandAsync("firewall-cmd", new[] { "--permanent", "--add-port=" + port });
            }
            await RunFirewallCommandAsync("firewall-cmd", new[] { "--reload" });
            return;
        }

        if (UnixProcessRunner.CommandExists("iptables"))
        {
            foreach (var port in Ports)
            {
                string proto = port.EndsWith("udp", StringComparison.Ordinal) ? "udp" : "tcp";
                string portNumber = port.Split('/')[0];
                await RunFirewallCommandAsync("iptables", new[] { "-A", "INPUT", "-p", proto, "--dport", portNumber, "-j", "ACCEPT" });
            }
            return;
        }

        AppLogger.Log(
            "[FIREWALL] No supported firewall tool found (ufw/firewall-cmd/iptables). " +
            $"Open UDP {Ports[0].Split('/')[0]}, TCP {Ports[1].Split('/')[0]} and TCP {Ports[2].Split('/')[0]} manually.");
    }

    private static async Task RunFirewallCommandAsync(string command, string[] args)
    {
        // As root run the tool directly; otherwise `sudo -n` (non-interactive — succeeds on
        // NOPASSWD, fails fast with a logged instruction otherwise; never blocks the app on
        // a password prompt).
        var result = PrivilegeEscalationHelper.IsRunningAsRoot()
            ? await Task.Run(() => UnixProcessRunner.Run(command, args))
            : await Task.Run(() => UnixProcessRunner.Run("sudo", new[] { "-n", command }.Concat(args)));

        string display = $"{command} {string.Join(' ', args)}";
        if (result.Succeeded)
        {
            AppLogger.Log($"[FIREWALL] {display} — ok.");
        }
        else
        {
            AppLogger.Error($"[FIREWALL] {display} failed (exit {result.ExitCode}): {result.StdErr.Trim()}");
            AppLogger.Log($"[FIREWALL] To add the rule manually, run: sudo {display}");
        }
    }
}
