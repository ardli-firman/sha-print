using System.Diagnostics;
using System.Net.Http;
using System.Text.Json;
using Microsoft.Extensions.Hosting;
using ShaPrint.Core;

namespace ShaPrint.UI.Services;

/// <summary>Release channel filter. Mirrors <c>ShaPrint.WpfApp.Models.UpdateChannel</c>; kept
/// local until the shared settings surface lands (Task 5).</summary>
public enum UpdateChannel
{
    Stable,
    Beta
}

public class GitHubRelease
{
    public string Name { get; set; } = string.Empty;
    public string TagName { get; set; } = string.Empty;
    public Version Version { get; set; } = new Version(0, 0, 0, 0);
    public UpdateChannel Channel { get; set; }
    public string DownloadUrl { get; set; } = string.Empty;
    public DateTime PublishedAt { get; set; }
}

/// <summary>
/// Snapshot of the update-related settings consumed by <see cref="UpdateService"/>'s
/// background auto-check. Durable persistence (the WPF <c>AppSettings</c> surface) is wired
/// by the SettingsViewModel in Task 5; until then the service defaults to
/// <see cref="DefaultProvider"/> (stable channel, auto-update on, check on every app start).
/// </summary>
public sealed record UpdateCheckSettings(UpdateChannel Channel, bool AutoUpdateEnabled, DateTime LastUpdateCheck);

/// <summary>Raised by the background auto-check when a newer release is found on the selected
/// channel. The UI layer decides how to prompt (WPF used a MessageBox).</summary>
public sealed class UpdateAvailableEventArgs
{
    public GitHubRelease Release { get; }

    public UpdateAvailableEventArgs(GitHubRelease release)
    {
        Release = release;
    }
}

/// <summary>
/// Shared GitHub-based update service. Migrated from
/// <c>ShaPrint.WpfApp/Services/UpdateService.cs</c> (Task 4) as an injectable
/// <see cref="IHostedService"/>. The WPF MessageBox prompt is replaced by the
/// <see cref="UpdateAvailable"/> event; the updater executable name is picked per-OS.
/// </summary>
public class UpdateService : IHostedService
{
    private const string RepoUrl = "https://api.github.com/repos/ardli-firman/sha-print/releases";

    private readonly Func<UpdateCheckSettings> _settingsProvider;
    private DateTime _lastCheck = DateTime.MinValue;

    public event EventHandler<UpdateAvailableEventArgs>? UpdateAvailable;

    /// <summary>Default provider: stable channel, auto-update enabled, never checked before
    /// (session-persisted <see cref="_lastCheck"/> applies the 6h gate within the session).</summary>
    public static Func<UpdateCheckSettings> DefaultProvider =>
        () => new UpdateCheckSettings(UpdateChannel.Stable, true, DateTime.MinValue);

    public UpdateService(Func<UpdateCheckSettings>? settingsProvider = null)
    {
        _settingsProvider = settingsProvider ?? DefaultProvider;
    }

    public Task StartAsync(CancellationToken cancellationToken)
    {
        // Run check in the background
        _ = Task.Run(CheckForUpdatesAsync, cancellationToken);
        return Task.CompletedTask;
    }

    public Task StopAsync(CancellationToken cancellationToken)
    {
        return Task.CompletedTask;
    }

    public async Task<List<GitHubRelease>> GetAvailableReleasesAsync()
    {
        var releases = new List<GitHubRelease>();
        try
        {
            using var client = new HttpClient();
            client.DefaultRequestHeaders.Add("User-Agent", "ShaPrint-App");

            var response = await client.GetAsync(RepoUrl);
            if (!response.IsSuccessStatusCode) return releases;

            var json = await response.Content.ReadAsStringAsync();
            using var doc = JsonDocument.Parse(json);

            foreach (var element in doc.RootElement.EnumerateArray())
            {
                string tagName = element.GetProperty("tag_name").GetString() ?? "";
                if (string.IsNullOrEmpty(tagName)) continue;

                string originalTag = tagName;
                UpdateChannel channel = UpdateChannel.Stable;
                int betaCounter = 0;

                if (tagName.StartsWith("v", StringComparison.OrdinalIgnoreCase))
                    tagName = tagName.Substring(1);

                // Parse tag format: 1.2.3-stable, 1.2.3-beta, 1.2.3-beta.1
                if (tagName.EndsWith("-stable", StringComparison.OrdinalIgnoreCase))
                {
                    tagName = tagName.Substring(0, tagName.Length - 7);
                    channel = UpdateChannel.Stable;
                }
                else if (tagName.Contains("-beta"))
                {
                    // Handle both "1.2.3-beta" and "1.2.3-beta.1"
                    int betaIndex = tagName.IndexOf("-beta");
                    string betaPart = tagName.Substring(betaIndex);
                    tagName = tagName.Substring(0, betaIndex);
                    channel = UpdateChannel.Beta;

                    // Extract beta counter if exists (e.g., "-beta.1" → 1)
                    if (betaPart.Contains("."))
                    {
                        string counterStr = betaPart.Substring(betaPart.LastIndexOf(".") + 1);
                        int.TryParse(counterStr, out betaCounter);
                    }
                }

                if (!Version.TryParse(tagName, out Version? parsedVersion)) continue;

                // Create version with beta counter as 4th component for proper comparison
                // 1.2.3-beta.1 → 1.2.3.1, 1.2.3-beta → 1.2.3.0
                if (channel == UpdateChannel.Beta)
                {
                    parsedVersion = new Version(parsedVersion.Major, parsedVersion.Minor, parsedVersion.Build, betaCounter);
                }

                string downloadUrl = "";
                if (element.TryGetProperty("assets", out var assets))
                {
                    foreach (var asset in assets.EnumerateArray())
                    {
                        string assetName = asset.GetProperty("name").GetString() ?? "";
                        if (assetName.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
                        {
                            downloadUrl = asset.GetProperty("browser_download_url").GetString() ?? "";
                            break;
                        }
                    }
                }

                if (string.IsNullOrEmpty(downloadUrl)) continue;

                DateTime publishedAt = DateTime.MinValue;
                if (element.TryGetProperty("published_at", out var publishedAtProp))
                {
                    publishedAtProp.TryGetDateTime(out publishedAt);
                }

                releases.Add(new GitHubRelease
                {
                    Name = element.GetProperty("name").GetString() ?? originalTag,
                    TagName = originalTag,
                    Version = parsedVersion,
                    Channel = channel,
                    DownloadUrl = downloadUrl,
                    PublishedAt = publishedAt
                });
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("Failed to fetch releases from GitHub.", ex);
        }
        return releases;
    }

    private async Task CheckForUpdatesAsync()
    {
        // Delay to prevent slowing down app startup
        await Task.Delay(5000);

        var settings = _settingsProvider();
        // The 6h gate covers both the persisted snapshot and this session's checks.
        DateTime lastCheck = settings.LastUpdateCheck > _lastCheck ? settings.LastUpdateCheck : _lastCheck;
        if (lastCheck.AddHours(6) > DateTime.Now)
        {
            return;
        }

        try
        {
            var releases = await GetAvailableReleasesAsync();
            _lastCheck = DateTime.Now;

            if (releases.Count == 0) return;

            var targetChannel = settings.Channel;
            var latestInChannel = releases
                .Where(r => r.Channel == targetChannel)
                .OrderByDescending(r => r.Version)
                .FirstOrDefault();

            if (latestInChannel == null) return;

            Version currentVersion = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version ?? new Version(0, 0, 0, 0);

            if (latestInChannel.Version > currentVersion)
            {
                if (settings.AutoUpdateEnabled)
                {
                    LaunchUpdater(latestInChannel.DownloadUrl);
                }
                else
                {
                    // UI layer decides how to notify the user (event instead of WPF MessageBox).
                    UpdateAvailable?.Invoke(this, new UpdateAvailableEventArgs(latestInChannel));
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("Failed to check for auto updates.", ex);
        }
    }

    public void LaunchUpdater(string downloadUrl)
    {
        string exeName = OperatingSystem.IsWindows() ? "ShaPrint.Updater.exe" : "ShaPrint.Updater";
        string updaterPath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, exeName);
        if (File.Exists(updaterPath))
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = updaterPath,
                Arguments = $"--url \"{downloadUrl}\"",
                UseShellExecute = true
            });
        }
        else
        {
            AppLogger.Log($"[UPDATER] Updater executable not found: {updaterPath}");
        }
    }
}