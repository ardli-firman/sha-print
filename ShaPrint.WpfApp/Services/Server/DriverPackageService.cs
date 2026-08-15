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
            AppLogger.Log($"[DRIVER_PKG] GetPackageDirectory: looking up '{driverPackageId[..16]}...' (cache size={_cache.Count})");

            // 1. Try in-memory cache first
            var entry = _cache.Values.FirstOrDefault(c =>
                c.Manifest.Sha256.Equals(driverPackageId, StringComparison.OrdinalIgnoreCase));
            if (entry != null)
            {
                if (!string.IsNullOrEmpty(entry.DirectoryPath) && _fileSystem.DirectoryExists(entry.DirectoryPath))
                {
                    AppLogger.Log($"[DRIVER_PKG] GetPackageDirectory: cache hit → {entry.DirectoryPath}");
                    return entry.DirectoryPath;
                }

                string dir = Path.Combine(_cacheRoot, entry.Manifest.Sha256);
                if (_fileSystem.DirectoryExists(dir))
                {
                    AppLogger.Log($"[DRIVER_PKG] GetPackageDirectory: cache hit (fallback dir) → {dir}");
                    return dir;
                }

                AppLogger.Log($"[DRIVER_PKG] GetPackageDirectory: cache entry found but directory missing — will try disk.");
            }
            else
            {
                AppLogger.Log($"[DRIVER_PKG] GetPackageDirectory: not in memory cache — trying disk fallback.");
            }

            // 2. Disk fallback: after server restart, cache is empty but files may exist
            string diskDir = Path.Combine(_cacheRoot, driverPackageId);
            string diskZip = Path.Combine(diskDir, "package.zip");
            if (_fileSystem.FileExists(diskZip))
            {
                AppLogger.Log($"[DRIVER_PKG] GetPackageDirectory: disk fallback hit → {diskDir}");
                return diskDir;
            }

            AppLogger.Error($"[DRIVER_PKG] GetPackageDirectory: '{driverPackageId[..16]}...' not found in cache or on disk.");
            return null;
        }

        /// <summary>
        /// Reads the zip archive bytes from the package directory, suitable for chunked transfer.
        /// The zip is created during ExportDriverAsync and contains all driver files.
        /// </summary>
        public async Task<byte[]?> ReadPackageBytesAsync(string driverPackageId)
        {
            AppLogger.Log($"[DRIVER_PKG] ReadPackageBytesAsync: requested '{driverPackageId[..16]}...'");
            string? dir = GetPackageDirectory(driverPackageId);
            if (dir == null)
            {
                AppLogger.Error($"[DRIVER_PKG] ReadPackageBytesAsync: no directory found for '{driverPackageId[..16]}...' — transfer aborted.");
                return null;
            }

            string zipPath = Path.Combine(dir, "package.zip");
            if (!_fileSystem.FileExists(zipPath))
            {
                AppLogger.Error($"[DRIVER_PKG] ReadPackageBytesAsync: package.zip not found in {dir}");
                return null;
            }

            try
            {
                long fileSize = _fileSystem.GetFileSize(zipPath);
                AppLogger.Log($"[DRIVER_PKG] ReadPackageBytesAsync: reading package.zip ({fileSize:N0} bytes) from {dir}");
                var bytes = await _fileSystem.ReadAllBytesAsync(zipPath);
                AppLogger.Log($"[DRIVER_PKG] ReadPackageBytesAsync: read {bytes.Length:N0} bytes OK.");
                return bytes;
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[DRIVER_PKG] ReadPackageBytesAsync: error reading package.zip for '{driverPackageId[..16]}...'", ex);
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
            AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: starting export for '{driverName}' (inf={infName})");

            // Compute a preliminary hash of the inf content for the cache directory name
            byte[] infBytes = await _fileSystem.ReadAllBytesAsync(infPath);
            string infHash = Convert.ToHexString(SHA256.HashData(infBytes)).ToLowerInvariant();

            string exportDir = Path.Combine(_cacheRoot, infHash);
            _fileSystem.CreateDirectory(exportDir);
            AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: exportDir='{exportDir}'");

            // Clean up stale meta-files from previous export runs to avoid them being zipped in
            foreach (var stale in new[] { "package.zip", "manifest.json" })
            {
                string stalePath = Path.Combine(exportDir, stale);
                if (_fileSystem.FileExists(stalePath))
                {
                    _fileSystem.DeleteFile(stalePath);
                    AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: deleted stale '{stale}' from exportDir.");
                }
            }

            // Export using pnputil
            AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: running pnputil /export-driver...");
            var result = await _processRunner.RunAsync("pnputil",
                $"/export-driver \"{infPath}\" \"{exportDir}\"",
                TimeSpan.FromMinutes(2));

            if (!result.Success)
            {
                AppLogger.Error($"[DRIVER_PKG] ExportDriverAsync: pnputil /export-driver failed for {infName}: {result.Output}");
                _fileSystem.DeleteDirectory(exportDir, true);
                return null;
            }
            AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: pnputil export succeeded.");

            // Build a zip archive — include only driver files (.inf, .cat, .dll, .gpd, .ppd)
            // Exclude meta-files we write ourselves (package.zip, manifest.json, .verified.json)
            try
            {
                var allFiles = _fileSystem.GetFiles(exportDir, "*", SearchOption.TopDirectoryOnly)
                    .OrderBy(f => f, StringComparer.OrdinalIgnoreCase)
                    .ToArray();

                // Filter: only real driver files, never our own packaging artifacts or Windows system INFs
                var driverExts = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
                    { ".inf", ".cat", ".dll", ".gpd", ".ppd", ".icm", ".oem" };
                var metaFiles = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
                    { "package.zip", "manifest.json", ".verified.json" };
                // Windows built-in INFs that pnputil co-exports as dependencies — exclude from package
                var windowsSystemInfs = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
                {
                    "ntprint.inf", "prnms001.inf", "prnms002.inf", "prnms003.inf",
                    "prnms006.inf", "prnms007.inf", "prnms008.inf", "prnms009.inf",
                    "prnms010.inf", "prnms011.inf", "prnms012.inf",
                    "usbprint.inf", "wsdprint.inf", "wsprint.inf",
                    "printqueue.inf", "prnroot.inf"
                };

                var files = allFiles
                    .Where(f => !metaFiles.Contains(Path.GetFileName(f))
                             && !windowsSystemInfs.Contains(Path.GetFileName(f))
                             && (driverExts.Contains(Path.GetExtension(f))
                                 || allFiles.Length <= 4)) // if very few files, include all non-meta
                    .ToArray();

                if (files.Length == 0)
                {
                    AppLogger.Error($"[DRIVER_PKG] ExportDriverAsync: no driver files found in exportDir after filter. allFiles={allFiles.Length}");
                    _fileSystem.DeleteDirectory(exportDir, true);
                    return null;
                }

                AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: zipping {files.Length} driver file(s): {string.Join(", ", files.Select(Path.GetFileName))}");

                // Create zip archive in memory from driver files
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

                // Compute SHA-256 from zip bytes (canonical package hash)
                string packageHash = Convert.ToHexString(SHA256.HashData(zipBytes)).ToLowerInvariant();
                AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: zip={zipBytes.Length:N0} bytes, SHA-256={packageHash[..16]}...");

                // Resolve actual INF filename from the exported files (NOT the Windows oem*.inf store name).
                // pnputil exports with the original manufacturer name (e.g., EPSONL3210.inf).
                // Priority: non-oem-numbered INF > oem-numbered INF > fallback to store name.
                string actualInfName;
                var infFiles = files
                    .Where(f => f.EndsWith(".inf", StringComparison.OrdinalIgnoreCase))
                    .Select(Path.GetFileName)
                    .ToArray();

                var nonOemInf = infFiles.FirstOrDefault(n =>
                    !string.IsNullOrEmpty(n) &&
                    !n.StartsWith("oem", StringComparison.OrdinalIgnoreCase));
                actualInfName = nonOemInf
                    ?? infFiles.FirstOrDefault()
                    ?? infName; // absolute fallback: Windows store name

                AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: resolved INF name for manifest: '{infName}' (store) → '{actualInfName}' (exported)");

                // Build manifest
                var manifest = new DriverPackageManifest
                {
                    InfName = actualInfName,
                    DriverName = driverName,
                    Sha256 = packageHash,
                    TotalSizeBytes = zipBytes.Length,
                    FileCount = files.Length,
                    ExportedAt = DateTime.UtcNow.ToString("o"),
                    WindowsVersion = Environment.OSVersion.Version.ToString()
                };

                string manifestJson = JsonSerializer.Serialize(manifest, new JsonSerializerOptions { WriteIndented = true });

                // Write to exportDir (infHash-based, for pnputil output)
                await _fileSystem.WriteAllBytesAsync(Path.Combine(exportDir, "package.zip"), zipBytes);
                await _fileSystem.WriteAllBytesAsync(Path.Combine(exportDir, "manifest.json"), Encoding.UTF8.GetBytes(manifestJson));

                // Write to finalDir (packageHash-based, where ReadPackageBytesAsync looks)
                string finalDir = Path.Combine(_cacheRoot, packageHash);
                if (!string.Equals(exportDir, finalDir, StringComparison.OrdinalIgnoreCase))
                {
                    AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: writing final package to SHA-256 dir → {finalDir}");
                    _fileSystem.CreateDirectory(finalDir);
                    foreach (var f in files)
                    {
                        string dest = Path.Combine(finalDir, Path.GetFileName(f));
                        var content = await _fileSystem.ReadAllBytesAsync(f);
                        await _fileSystem.WriteAllBytesAsync(dest, content);
                    }
                    await _fileSystem.WriteAllBytesAsync(Path.Combine(finalDir, "package.zip"), zipBytes);
                    await _fileSystem.WriteAllBytesAsync(Path.Combine(finalDir, "manifest.json"), Encoding.UTF8.GetBytes(manifestJson));
                }
                else
                {
                    AppLogger.Log($"[DRIVER_PKG] ExportDriverAsync: exportDir matches packageHash dir — no copy needed.");
                }

                AppLogger.Log($"[DRIVER_PKG] Exported driver '{driverName}' → {files.Length} files, zip {zipBytes.Length:N0} bytes, SHA-256={packageHash[..16]}...");
                return manifest;
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[DRIVER_PKG] ExportDriverAsync: error building manifest for '{driverName}'", ex);
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
