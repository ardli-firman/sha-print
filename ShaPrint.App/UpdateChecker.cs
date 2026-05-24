using System;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Text.Json;
using System.Threading.Tasks;
using System.Windows.Forms;
using ShaPrint.Core;

namespace ShaPrint.App
{
    public static class UpdateChecker
    {
        private const string RepoUrl = "https://api.github.com/repos/ardli-firman/sha-print/releases/latest";

        public static async Task CheckForUpdatesAsync()
        {
            await PerformCheckAsync(isManual: false);
        }

        public static async Task CheckForUpdatesManualAsync()
        {
            await PerformCheckAsync(isManual: true);
        }

        private static async Task PerformCheckAsync(bool isManual)
        {
            // Mencegah rate-limit GitHub (60 requests/hour/IP) dengan interval pengecekan 6 jam
            // Kecuali jika ini adalah pengecekan manual dari user
            if (!isManual && AppSettings.Current.LastUpdateCheck.AddHours(6) > DateTime.Now)
            {
                return;
            }

            try
            {
                using var client = new HttpClient();
                client.DefaultRequestHeaders.Add("User-Agent", "ShaPrint-App");
                
                var response = await client.GetAsync(RepoUrl);
                
                // Update waktu terakhir pengecekan terlepas dari berhasil atau gagalnya request (termasuk saat kena rate limit 403)
                AppSettings.Current.LastUpdateCheck = DateTime.Now;
                AppSettings.Save();

                if (!response.IsSuccessStatusCode) return;

                var json = await response.Content.ReadAsStringAsync();
                using var doc = JsonDocument.Parse(json);
                
                string tagName = doc.RootElement.GetProperty("tag_name").GetString() ?? "";
                if (string.IsNullOrEmpty(tagName)) return;

                // Strip 'v' prefix if present
                if (tagName.StartsWith("v", StringComparison.OrdinalIgnoreCase))
                    tagName = tagName.Substring(1);

                // Strip '-prod' suffix if present
                if (tagName.EndsWith("-prod", StringComparison.OrdinalIgnoreCase))
                    tagName = tagName.Substring(0, tagName.Length - 5);

                Version latestVersion;
                if (!Version.TryParse(tagName, out latestVersion)) return;

                Version currentVersion = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version;

                if (latestVersion > currentVersion)
                {
                    // Find asset download url
                    string downloadUrl = "";
                    var assets = doc.RootElement.GetProperty("assets");
                    foreach (var asset in assets.EnumerateArray())
                    {
                        string name = asset.GetProperty("name").GetString() ?? "";
                        if (name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
                        {
                            downloadUrl = asset.GetProperty("browser_download_url").GetString() ?? "";
                            break;
                        }
                    }

                    if (!string.IsNullOrEmpty(downloadUrl))
                    {
                        if (!isManual && AppSettings.Current.AutoUpdateEnabled)
                        {
                            LaunchUpdaterAndExit(downloadUrl);
                        }
                        else
                        {
                            var result = MessageBox.Show($"A new version of ShaPrint ({latestVersion}) is available.\nWould you like to update now?", 
                                "Update Available", MessageBoxButtons.YesNo, MessageBoxIcon.Information);
                                
                            if (result == DialogResult.Yes)
                            {
                                LaunchUpdaterAndExit(downloadUrl);
                            }
                        }
                    }
                }
                else if (isManual)
                {
                    MessageBox.Show("You are already using the latest version.", "Up to Date", MessageBoxButtons.OK, MessageBoxIcon.Information);
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("Failed to check for updates.", ex);
                if (isManual) 
                {
                    MessageBox.Show("Failed to check for updates. Please check your internet connection.", "Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
                }
            }
        }

        private static void LaunchUpdaterAndExit(string downloadUrl)
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
