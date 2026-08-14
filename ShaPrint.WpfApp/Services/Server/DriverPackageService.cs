using System;
using System.Collections.Concurrent;
using System.IO;
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
                string dir = Path.Combine(_cacheRoot, entry.Manifest.Sha256);
                if (_fileSystem.DirectoryExists(dir)) return dir;
            }
            return null;
        }

        /// <summary>
        /// Reads all file bytes concatenated from the package directory, suitable for chunked transfer.
        /// </summary>
        public async Task<byte[]?> ReadPackageBytesAsync(string driverPackageId)
        {
            string? dir = GetPackageDirectory(driverPackageId);
            if (dir == null) return null;

            try
            {
                var files = _fileSystem.GetFiles(dir, "*", SearchOption.TopDirectoryOnly)
                    .Where(f => !Path.GetFileName(f).Equals("manifest.json", StringComparison.OrdinalIgnoreCase))
                    .OrderBy(f => f, StringComparer.OrdinalIgnoreCase)
                    .ToArray();

                using var ms = new MemoryStream();
                foreach (var f in files)
                {
                    var bytes = await _fileSystem.ReadAllBytesAsync(f);
                    await ms.WriteAsync(bytes);
                }
                return ms.ToArray();
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[DRIVER_PKG] Error reading package bytes for {driverPackageId}", ex);
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
        /// Exports a driver package using pnputil /export-driver and builds the manifest.
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

            // Compute total hash of all exported files (concatenated, sorted by name)
            try
            {
                var files = _fileSystem.GetFiles(exportDir, "*", SearchOption.TopDirectoryOnly)
                    .OrderBy(f => f, StringComparer.OrdinalIgnoreCase)
                    .ToArray();

                long totalSize = 0;
                using var sha = SHA256.Create();
                using var hashStream = new CryptoStream(Stream.Null, sha, CryptoStreamMode.Write);

                foreach (var f in files)
                {
                    var bytes = await _fileSystem.ReadAllBytesAsync(f);
                    totalSize += bytes.Length;
                    await hashStream.WriteAsync(bytes);
                }
                await hashStream.FlushFinalBlockAsync();

                string packageHash = Convert.ToHexString(sha.Hash!).ToLowerInvariant();

                // Build manifest
                var manifest = new DriverPackageManifest
                {
                    InfName = infName,
                    DriverName = driverName,
                    Sha256 = packageHash,
                    TotalSizeBytes = totalSize,
                    FileCount = files.Length,
                    ExportedAt = DateTime.UtcNow.ToString("o"),
                    WindowsVersion = Environment.OSVersion.Version.ToString()
                };

                // Write manifest.json
                string manifestJson = JsonSerializer.Serialize(manifest, new JsonSerializerOptions { WriteIndented = true });
                await _fileSystem.WriteAllBytesAsync(
                    Path.Combine(exportDir, "manifest.json"),
                    Encoding.UTF8.GetBytes(manifestJson));

                AppLogger.Log($"[DRIVER_PKG] Exported driver '{driverName}' → {files.Length} files, {totalSize} bytes, SHA-256={packageHash[..16]}...");

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
            public DateTime ExportedAt { get; set; }
        }
    }
}
