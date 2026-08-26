namespace ShaPrint.Platform.Abstractions;

public interface IFirewallManager
{
    Task EnsureFirewallRulesAsync();
}