using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Text.Json;
using System.Threading.Tasks;
using System.Windows.Forms;
using System.Linq;
using ShaPrint.Core;

namespace ShaPrint.App
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

    public static class UpdateChecker
    {
        private const string RepoUrl = "https://api.github.com/repos/ardli-firman/sha-print/releases";

        public static async Task<List<GitHubRelease>> GetAvailableReleasesAsync()
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

                    if (tagName.StartsWith("v", StringComparison.OrdinalIgnoreCase))
                        tagName = tagName.Substring(1);

                    if (tagName.EndsWith("-stable", StringComparison.OrdinalIgnoreCase))
                    {
                        tagName = tagName.Substring(0, tagName.Length - 7);
                        channel = UpdateChannel.Stable;
                    }
                    else if (tagName.EndsWith("-beta", StringComparison.OrdinalIgnoreCase))
                    {
                        tagName = tagName.Substring(0, tagName.Length - 5);
                        channel = UpdateChannel.Beta;
                    }
                    else
                    {
                        // Fallback, consider plain tags as stable
                        channel = UpdateChannel.Stable;
                    }

                    if (!Version.TryParse(tagName, out Version? parsedVersion)) continue;

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

        public static async Task CheckForUpdatesAsync()
        {
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
                        var result = MessageBox.Show($"A new version of ShaPrint ({latestInChannel.Version} - {latestInChannel.Channel}) is available.\nWould you like to open the Update Manager?", 
                            "Update Available", MessageBoxButtons.YesNo, MessageBoxIcon.Information);
                            
                        if (result == DialogResult.Yes)
                        {
                            CheckForUpdatesManualAsync().GetAwaiter().GetResult();
                        }
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("Failed to check for auto updates.", ex);
            }
        }

        public static async Task CheckForUpdatesManualAsync()
        {
            // Open the UpdateManagerForm
            var form = new UpdateManagerForm();
            form.ShowDialog();
            await Task.CompletedTask;
        }

        public static void LaunchUpdaterAndExit(string downloadUrl)
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
                MessageBox.Show("Updater executable not found.", "Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }
    }
}
