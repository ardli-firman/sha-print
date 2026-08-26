using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Windows.Adapters;

/// <summary>
/// Adapter exposing the Task Scheduler-based <see cref="StartupManager"/> through the
/// platform-agnostic <see cref="IStartupManager"/> interface.
/// </summary>
public sealed class WindowsStartupManager : IStartupManager
{
    public void SetStartup(bool enable)
        => StartupManager.SetStartup(enable);

    public bool IsStartupEnabled()
        => StartupManager.IsStartupEnabled();
}