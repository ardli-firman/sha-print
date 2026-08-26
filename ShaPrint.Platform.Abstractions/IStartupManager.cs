namespace ShaPrint.Platform.Abstractions;

public interface IStartupManager
{
    void SetStartup(bool enable);
    bool IsStartupEnabled();
}