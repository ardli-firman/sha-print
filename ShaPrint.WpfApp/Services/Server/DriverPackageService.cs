using System;
using System.Collections.Concurrent;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Abstractions;
using ShaPrint.Core.Network;

namespace ShaPrint.Server
{
    /// <summary>
    /// Server-side service that locates, exports, and caches driver packages
    /// for shared printers using pnputil /export-driver.
    /// Cache invalidated by PrintService event log events + 24h TTL fallback.
    /// </summary>
    public class DriverPackageService
    {
        private readonly IProcessRunner _processRunner;
        private readonly IFileSystem _fileSystem;
        private readonly IEventLog? _eventLog;

        // Cache: driverName → CachedPackage
        private readonly ConcurrentDictionary<string, CachedPackage> _cache = new(StringComparer.OrdinalIgnoreCase);

        private readonly string _cacheRoot;
        private static readonly TimeSpan CacheTtl = TimeSpan.FromHours(Constants.DriverPackageCacheTtlHours);

        public DriverPackageService(
            IProcessRunner processRunner,
            IFileSystem fileSystem,
            IEventLog? eventLog = null)
        {
            _processRunner = processRunner;
            _fileSystem = fileSystem;
            _eventLog = eventLog;
            _cacheRoot = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "ShaPrint", "DriverCache");
        }

        /// <summary>
        /// Gets (or builds) the driver package metadata for a printer.
        /// Returns null if the driver cannot be located or exported.
        /// </summary>
        public async Task<DriverPackageManifest?> GetDriverPackageAsync(string driverName)
        {
            if (string.IsNullOrWhiteSpace(driverName)) return null;

            // Check cache first
            if (_cache.TryGetValue(driverName, out var cached))
            {
                if (!IsCacheStale(cached))
                    return cached.Manifest;

                // Stale — evict and re-export
                _cache.TryRemove(driverName, out _);
            }

            // Locate the .inf path for this driver
            string? infPath = await LocateInfPathAsync(driverName);
            if (infPath == null)
            {
                AppLogger.Log($"[DRIVER_PKG] Could not locate .inf for driver '{driverName}'.");
                return null;
            }

            // Export the driver
            var manifest = await ExportDriverAsync(infPath, driverName);
            if (manifest != null)
            {
                _cache[driverName] = new CachedPackage
                {
                    Manifest = manifest,
                    DirectoryPath = Path.Combine(_cacheRoot, manifest.Sha256),
                    ExportedAt = DateTime.UtcNow
                };
            }
            return manifest;
        }

        /// <summary>
        /// Gets the cached directory path for a driver package by its SHA-256 hash.
        /// Returns null if not cached.
        /// </summary>
        public string? GetPackageDirectory(string driverPackageId)
        {
            var entry = _cache.Values.FirstOrDefault(c =>
                c.Manifest.Sha256.Equals(driverPackageId, StringComparison.OrdinalIgnoreCase));
            if (entry != null)
            {
                if (!string.IsNullOrEmpty(entry.DirectoryPath) && _fileSystem.DirectoryExists(entry.DirectoryPath))
                    return entry.DirectoryPath;

                string dir = Path.Combine(_cacheRoot, entry.Manifest.Sha256);
                if (_fileSystem.DirectoryExists(dir)) return dir;
            }
            return null;
        }

        /// <summary>
        /// Reads the zip archive bytes from the package directory, suitable for chunked transfer.
        /// The zip is created during ExportDriverAsync and contains all driver files.
        /// </summary>
        public async Task<byte[]?> ReadPackageBytesAsync(string driverPackageId)
        {
            string? dir = GetPackageDirectory(driverPackageId);
            if (dir == null) return null;

            string zipPath = Path.Combine(dir, "package.zip");
            if (!_fileSystem.FileExists(zipPath))
            {
                AppLogger.Error($"[DRIVER_PKG] package.zip not found in {dir}");
                return null;
            }

            try
            {
                return await _fileSystem.ReadAllBytesAsync(zipPath);
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[DRIVER_PKG] Error reading package.zip for {driverPackageId}", ex);
                return null;
            }
        }

        /// <summary>
        /// Locates the .inf file path for a given driver name using WMI (via PowerShell).
        /// Returns e.g. "C:\Windows\INF\oem25.inf" or null if not found.
        /// </summary>
        private async Task<string?> LocateInfPathAsync(string driverName)
        {
            // Escape single quotes for safe PowerShell embedding
            string safeName = driverName.Replace("'", "''");

            // Try WMI to get the InfPath
            var result = await _processRunner.RunAsync("powershell.exe",
                $"-NoProfile -ExecutionPolicy Bypass -Command \"Get-WmiObject Win32_PrinterDriver | Where-Object {{ $_.Name -like '*{safeName}*' }} | Select-Object -First 1 -ExpandProperty InfPath 2>&1 | Out-String -Width 4096\"",
                TimeSpan.FromSeconds(30));

            if (result.Success && !string.IsNullOrWhiteSpace(result.Output))
            {
                string infPath = result.Output.Trim();
                // Remove any whitespace/newlines
                infPath = infPath.Split('\n', '\r').First().Trim();
                if (_fileSystem.FileExists(infPath))
                    return infPath;
            }

            // Fallback: try pnputil /enum-drivers and parse
            var pnputilResult = await _processRunner.RunAsync("pnputil",
                "/enum-drivers /class printer",
                TimeSpan.FromSeconds(30));

            if (pnputilResult.Success)
            {
                // Look for the inf name in the output
                string output = pnputilResult.Output;
                // This is a heuristic — look for "oem*.inf" lines that are near the driver name
                var lines = output.Split('\n');
                for (int i = 0; i < lines.Length; i++)
                {
                    if (lines[i].Contains("Original Name", StringComparison.OrdinalIgnoreCase))
                    {
                        string infName = lines[i].Split(':').Last().Trim();
                        if (infName.EndsWith(".inf", StringComparison.OrdinalIgnoreCase))
                        {
                            string fullPath = Path.Combine(
                                Environment.GetFolderPath(Environment.SpecialFolder.Windows),
                                "INF", infName);
                            if (_fileSystem.FileExists(fullPath))
                                return fullPath;
                        }
                    }
                }
            }

            return null;
        }

        /// <summary>
        /// Exports a driver package using pnputil /export-driver, creates a zip archive,
        /// and builds the manifest with SHA-256 computed from the zip bytes.
        /// </summary>
        private async Task<DriverPackageManifest?> ExportDriverAsync(string infPath, string driverName)
        {
            string infName = Path.GetFileName(infPath);

            // Compute a preliminary hash of the inf content for the cache directory name
            byte[] infBytes = await _fileSystem.ReadAllBytesAsync(infPath);
            string infHash = Convert.ToHexString(SHA256.HashData(infBytes)).ToLowerInvariant();

            string exportDir = Path.Combine(_cacheRoot, infHash);
            _fileSystem.CreateDirectory(exportDir);

            // Export using pnputil
            var result = await _processRunner.RunAsync("pnputil",
                $"/export-driver \"{infPath}\" \"{exportDir}\"",
                TimeSpan.FromMinutes(2));

            if (!result.Success)
            {
                AppLogger.Error($"[DRIVER_PKG] pnputil /export-driver failed for {infName}: {result.Output}");
                _fileSystem.DeleteDirectory(exportDir, true);
                return null;
            }

            // Build a zip archive from the exported files and compute hash from zip bytes
            try
            {
                var files = _fileSystem.GetFiles(exportDir, "*", SearchOption.TopDirectoryOnly)
                    .OrderBy(f => f, StringComparer.OrdinalIgnoreCase)
                    .ToArray();

                // Create zip archive in memory from exported files
                byte[] zipBytes;
                using (var zipMs = new MemoryStream())
                {
                    using (var archive = new ZipArchive(zipMs, ZipArchiveMode.Create, leaveOpen: true))
                    {
                        foreach (var f in files)
                        {
                            string entryName = Path.GetFileName(f);
                            var entry = archive.CreateEntry(entryName, CompressionLevel.Optimal);
                            using var entryStream = entry.Open();
                            var fileBytes = await _fileSystem.ReadAllBytesAsync(f);
                            await entryStream.WriteAsync(fileBytes);
                        }
                    }
                    zipBytes = zipMs.ToArray();
                }

                // Write zip to export dir for caching (ReadPackageBytesAsync reads this)
                string zipPath = Path.Combine(exportDir, "package.zip");
                await _fileSystem.WriteAllBytesAsync(zipPath, zipBytes);

                // Compute SHA-256 from zip bytes (this is the canonical package hash)
                string packageHash = Convert.ToHexString(SHA256.HashData(zipBytes)).ToLowerInvariant();

                // Build manifest — hash and size refer to the zip
                var manifest = new DriverPackageManifest
                {
                    InfName = infName,
                    DriverName = driverName,
                    Sha256 = packageHash,
                    TotalSizeBytes = zipBytes.Length,
                    FileCount = files.Length,
                    ExportedAt = DateTime.UtcNow.ToString("o"),
                    WindowsVersion = Environment.OSVersion.Version.ToString()
                };

                // Write manifest.json
                string manifestJson = JsonSerializer.Serialize(manifest, new JsonSerializerOptions { WriteIndented = true });
                await _fileSystem.WriteAllBytesAsync(
                    Path.Combine(exportDir, "manifest.json"),
                    Encoding.UTF8.GetBytes(manifestJson));

                // Ensure final directory named with packageHash exists with package.zip and manifest.json
                string finalDir = Path.Combine(_cacheRoot, packageHash);
                if (!string.Equals(exportDir, finalDir, StringComparison.OrdinalIgnoreCase))
                {
                    _fileSystem.CreateDirectory(finalDir);
                    foreach (var f in files)
                    {
                        string dest = Path.Combine(finalDir, Path.GetFileName(f));
                        var content = await _fileSystem.ReadAllBytesAsync(f);
                        await _fileSystem.WriteAllBytesAsync(dest, content);
                    }
                    string finalZipPath = Path.Combine(finalDir, "package.zip");
                    await _fileSystem.WriteAllBytesAsync(finalZipPath, zipBytes);
                    await _fileSystem.WriteAllBytesAsync(
                        Path.Combine(finalDir, "manifest.json"),
                        Encoding.UTF8.GetBytes(manifestJson));
                }

                AppLogger.Log($"[DRIVER_PKG] Exported driver '{driverName}' → {files.Length} files, zip {zipBytes.Length} bytes, SHA-256={packageHash[..16]}...");

                return manifest;
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[DRIVER_PKG] Error building manifest for {driverName}", ex);
                return null;
            }
        }

        /// <summary>
        /// Checks if a cached entry is stale (older than TTL).
        /// </summary>
        private bool IsCacheStale(CachedPackage entry)
        {
            return (DateTime.UtcNow - entry.ExportedAt) > CacheTtl;
        }

        /// <summary>
        /// Forces invalidation of all cached driver packages.
        /// Called when a PrintService driver event is detected.
        /// </summary>
        public void InvalidateAll()
        {
            _cache.Clear();
            AppLogger.Log("[DRIVER_PKG] Cache invalidated (all entries cleared).");
        }

        /// <summary>
        /// Invalidates cache for a specific driver name.
        /// </summary>
        public void Invalidate(string driverName)
        {
            if (_cache.TryRemove(driverName, out _))
            {
                AppLogger.Log($"[DRIVER_PKG] Cache invalidated for driver '{driverName}'.");
            }
        }

        private class CachedPackage
        {
            public DriverPackageManifest Manifest { get; set; } = new();
            public string DirectoryPath { get; set; } = string.Empty;
            public DateTime ExportedAt { get; set; }
        }
    }
}
