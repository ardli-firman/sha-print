using Microsoft.Extensions.Hosting;
using ShaPrint.Core;
using ShaPrint.WpfApp.Models;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;

namespace ShaPrint.WpfApp.Services
{
    public class GitHubRelease
    {
        public string Name { get; set; } = string.Empty;
        public string TagName { get; set; } = string.Empty;
        public Version Version { get; set; } = new Version(0, 0, 0, 0);
        public UpdateChannel Channel { get; set; }
        public string DownloadUrl { get; set; } = string.Empty;
        public DateTime PublishedAt { get; set; }
    }

    public class UpdateService : IHostedService
    {
        private const string RepoUrl = "https://api.github.com/repos/ardli-firman/sha-print/releases";

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

            if (AppSettings.Current.LastUpdateCheck.AddHours(6) > DateTime.Now)
            {
                return;
            }

            try
            {
                var releases = await GetAvailableReleasesAsync();
                AppSettings.Current.LastUpdateCheck = DateTime.Now;
                AppSettings.Save();

                if (releases.Count == 0) return;

                var targetChannel = AppSettings.Current.Channel;
                var latestInChannel = releases
                    .Where(r => r.Channel == targetChannel)
                    .OrderByDescending(r => r.Version)
                    .FirstOrDefault();

                if (latestInChannel == null) return;

                Version currentVersion = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version ?? new Version(0, 0, 0, 0);

                if (latestInChannel.Version > currentVersion)
                {
                    if (AppSettings.Current.AutoUpdateEnabled)
                    {
                        LaunchUpdaterAndExit(latestInChannel.DownloadUrl);
                    }
                    else
                    {
                        Application.Current.Dispatcher.Invoke(() =>
                        {
                            var result = MessageBox.Show($"A new version of ShaPrint ({latestInChannel.Version} - {latestInChannel.Channel}) is available.\nWould you like to install it now?", 
                                "Update Available", MessageBoxButton.YesNo, MessageBoxImage.Information);
                                
                            if (result == MessageBoxResult.Yes)
                            {
                                LaunchUpdaterAndExit(latestInChannel.DownloadUrl);
                            }
                        });
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("Failed to check for auto updates.", ex);
            }
        }

        public void LaunchUpdaterAndExit(string downloadUrl)
        {
            string updaterPath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "ShaPrint.Updater.exe");
            if (File.Exists(updaterPath))
            {
                Process.Start(new ProcessStartInfo
                {
                    FileName = updaterPath,
                    Arguments = $"--url \"{downloadUrl}\"",
                    UseShellExecute = true
                });
                Environment.Exit(0);
            }
            else
            {
                Application.Current.Dispatcher.Invoke(() =>
                {
                    MessageBox.Show("Updater executable not found.", "Error", MessageBoxButton.OK, MessageBoxImage.Error);
                });
            }
        }
    }
}
