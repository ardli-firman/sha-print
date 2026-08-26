using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Windows.Adapters;

/// <summary>
/// Adapter exposing the netsh-based <see cref="FirewallManager"/> through the
/// platform-agnostic <see cref="IFirewallManager"/> interface.
///
/// Mirrors the original WPF call site (ServerViewModel.StartServer →
/// <c>FirewallManager.CheckAndAddFirewallRules()</c>): the underlying manager schedules
/// its own background work and returns immediately, so this returns a completed task.
/// </summary>
public sealed class WindowsFirewallManager : IFirewallManager
{
    public Task EnsureFirewallRulesAsync()
    {
        FirewallManager.EnsureFirewallRules();
        return Task.CompletedTask;
    }
}