using System;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.WpfApp.Services.Client
{
    /// <summary>
    /// Client-side service that downloads driver packages from the server,
    /// verifies integrity (SHA-256 + size), and caches them locally.
    /// Cache uses LRU eviction with a 500 MB cap.
    /// </summary>
    public class DriverPackageManager
    {
        private readonly string _cacheRoot;
        private static readonly SemaphoreSlim _downloadLock = new(1, 1);

        public DriverPackageManager()
        {
            _cacheRoot = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "ShaPrint", "DriverCache");
        }

        /// <summary>
        /// Downloads a driver package from the server and caches it.
        /// Verifies SHA-256 + size before returning.
        /// Returns the path to the extracted driver directory, or null on failure.
        /// </summary>
        public async Task<DriverDownloadResult> DownloadDriverPackageAsync(
            string serverIp,
            string printerName,
            string expectedPackageId,
            long expectedSize,
            IProgress<double>? progress = null,
            CancellationToken cancellationToken = default)
        {
            // H5: Size-cap pre-check
            if (expectedSize <= 0 || expectedSize > Constants.MaxDriverPackageSize)
            {
                return new DriverDownloadResult
                {
                    Success = false,
                    ErrorMessage = $"Driver package size ({expectedSize:N0} bytes) exceeds limit ({Constants.MaxDriverPackageSize / (1024 * 1024)} MB)."
                };
            }

            // H3: Cache check with re-verification via .verified.json
            string cachedDir = Path.Combine(_cacheRoot, expectedPackageId);
            if (Directory.Exists(cachedDir))
            {
                string markerPath = Path.Combine(cachedDir, ".verified.json");
                if (File.Exists(markerPath))
                {
                    try
                    {
                        var markerJson = File.ReadAllText(markerPath);
                        var marker = JsonSerializer.Deserialize<DriverPackageVerifiedMarker>(markerJson);
                        if (marker != null &&
                            marker.Sha256.Equals(expectedPackageId, StringComparison.OrdinalIgnoreCase))
                        {
                            AppLogger.Log($"[DRIVER_PKG_CLIENT] Cache verified: {expectedPackageId[..16]}...");
                            progress?.Report(1.0);
                            return new DriverDownloadResult
                            {
                                Success = true,
                                PackageDirectory = cachedDir,
                                FromCache = true
                            };
                        }
                        AppLogger.Log($"[DRIVER_PKG_CLIENT] Cache marker mismatch — re-downloading.");
                    }
                    catch (Exception ex)
                    {
                        AppLogger.Log($"[DRIVER_PKG_CLIENT] Cache marker unreadable — re-downloading: {ex.Message}");
                    }
                }
                else
                {
                    AppLogger.Log($"[DRIVER_PKG_CLIENT] Legacy cache (no .verified.json) — re-downloading once.");
                }
                // Marker missing or mismatched → delete stale cache entry
                try { Directory.Delete(cachedDir, true); } catch { }
            }

            // Acquire download semaphore (only one concurrent download)
            if (!await _downloadLock.WaitAsync(TimeSpan.FromSeconds(10), cancellationToken))
            {
                return new DriverDownloadResult { Success = false, ErrorMessage = "Download already in progress." };
            }

            try
            {
                return await DownloadFromServerAsync(serverIp, printerName, expectedPackageId, expectedSize, progress, cancellationToken);
            }
            finally
            {
                _downloadLock.Release();
            }
        }

        private async Task<DriverDownloadResult> DownloadFromServerAsync(
            string serverIp,
            string printerName,
            string expectedPackageId,
            long expectedSize,
            IProgress<double>? progress,
            CancellationToken cancellationToken)
        {
            string tempDir = Path.Combine(_cacheRoot, $"tmp_{Guid.NewGuid():N}");
            Directory.CreateDirectory(tempDir);

            try
            {
                using var client = new TcpClient();
                AppLogger.Log($"[DRIVER_PKG_CLIENT] Connecting to {serverIp}:{Constants.PrintTcpPort}...");
                var connectTask = client.ConnectAsync(serverIp, Constants.PrintTcpPort);
                if (await Task.WhenAny(connectTask, Task.Delay(Constants.DriverPackageTransferTimeoutMs, cancellationToken)) != connectTask)
                {
                    AppLogger.Error($"[DRIVER_PKG_CLIENT] Connection timeout to {serverIp}:{Constants.PrintTcpPort}");
                    return new DriverDownloadResult { Success = false, ErrorMessage = "Connection timeout." };
                }
                await connectTask;
                AppLogger.Log($"[DRIVER_PKG_CLIENT] Connected. Sending DriverPackageRequest for printer='{printerName}', PackageId={expectedPackageId[..16]}...");

                using var stream = client.GetStream();

                // Send DriverPackageRequest
                var request = new DriverPackageRequest
                {
                    PrinterName = printerName,
                    DriverPackageId = expectedPackageId
                };
                byte[] requestJson = JsonSerializer.SerializeToUtf8Bytes(request);

                // Write packet type + length-prefixed JSON
                using (var bw = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true))
                {
                    bw.Write(Constants.PacketTypeDriverPackageRequest);
                    bw.Write(requestJson.Length);
                    bw.Write(requestJson);
                }
                await stream.FlushAsync(cancellationToken);

                // Receive chunks
                byte[] allBytes;
                DriverPackageComplete? completeMessage = null;
                using (var ms = new MemoryStream())
                {
                    int totalChunks = 0;
                    int receivedChunks = 0;

                    while (true)
                    {
                        cancellationToken.ThrowIfCancellationRequested();

                        var packetType = await ReadInt32Async(stream, cancellationToken);
                        var payloadLength = await ReadInt32Async(stream, cancellationToken);

                        if (payloadLength <= 0 || payloadLength > Constants.MaxDriverPackageSize)
                        {
                            return new DriverDownloadResult
                            {
                                Success = false,
                                ErrorMessage = $"Invalid payload length: {payloadLength}"
                            };
                        }

                        // H7: Per-chunk sanity cap — reject oversized chunk payload
                        if (packetType == Constants.PacketTypeDriverPackageChunk &&
                            payloadLength > Constants.DriverPackageChunkSize * 2)
                        {
                            return new DriverDownloadResult
                            {
                                Success = false,
                                ErrorMessage = $"Chunk too large: {payloadLength} bytes (max {Constants.DriverPackageChunkSize * 2})."
                            };
                        }

                        byte[] payload = await ReadBytesAsync(stream, payloadLength, cancellationToken);

                        if (packetType == Constants.PacketTypeDriverPackageChunk)
                        {
                            var chunk = JsonSerializer.Deserialize<DriverPackageChunk>(
                                Encoding.UTF8.GetString(payload));
                            if (chunk == null)
                            {
                                return new DriverDownloadResult { Success = false, ErrorMessage = "Invalid chunk data." };
                            }

                            // Decrypt the chunk
                            byte[] rawChunk = CryptoHelper.DecryptAesGcm(chunk.Data);

                            // Verify per-chunk HMAC
                            if (!CryptoHelper.VerifyHmac(rawChunk, chunk.ChunkHmac))
                            {
                                return new DriverDownloadResult
                                {
                                    Success = false,
                                    ErrorMessage = $"Chunk {chunk.ChunkIndex} HMAC verification failed."
                                };
                            }

                            await ms.WriteAsync(rawChunk, cancellationToken);
                            receivedChunks++;
                            totalChunks = chunk.TotalChunks;
                            AppLogger.Log($"[DRIVER_PKG_CLIENT] Chunk {chunk.ChunkIndex + 1}/{totalChunks} received ({rawChunk.Length:N0} bytes).");
                            progress?.Report((double)receivedChunks / totalChunks);
                        }
                        else if (packetType == Constants.PacketTypeDriverPackageComplete)
                        {
                            completeMessage = JsonSerializer.Deserialize<DriverPackageComplete>(
                                Encoding.UTF8.GetString(payload));
                            if (completeMessage == null)
                            {
                                return new DriverDownloadResult { Success = false, ErrorMessage = "Invalid completion message." };
                            }

                            allBytes = ms.ToArray();
                            AppLogger.Log($"[DRIVER_PKG_CLIENT] All chunks received: {allBytes.Length:N0} bytes total (expected={expectedSize:N0}).");

                            // Verify total size — skip if expectedSize is 0 (stale discovery metadata)
                            if (expectedSize > 0 && allBytes.Length != expectedSize)
                            {
                                AppLogger.Error($"[DRIVER_PKG_CLIENT] Size mismatch: expected {expectedSize:N0}, got {allBytes.Length:N0}");
                                return new DriverDownloadResult
                                {
                                    Success = false,
                                    ErrorMessage = $"Size mismatch: expected {expectedSize}, got {allBytes.Length}"
                                };
                            }

                            // H5: Cross-check server-reported total bytes
                            if (expectedSize > 0 && completeMessage.TotalBytes != expectedSize)
                            {
                                AppLogger.Error($"[DRIVER_PKG_CLIENT] Server-reported size mismatch: server={completeMessage.TotalBytes:N0}, expected={expectedSize:N0}");
                                return new DriverDownloadResult
                                {
                                    Success = false,
                                    ErrorMessage = $"Server-reported size mismatch: server says {completeMessage.TotalBytes}, expected {expectedSize}"
                                };
                            }

                            // Verify SHA-256
                            string actualHash = Convert.ToHexString(SHA256.HashData(allBytes)).ToLowerInvariant();
                            AppLogger.Log($"[DRIVER_PKG_CLIENT] SHA-256 verify: expected={expectedPackageId[..16]}..., got={actualHash[..16]}...");
                            if (!actualHash.Equals(expectedPackageId, StringComparison.OrdinalIgnoreCase))
                            {
                                AppLogger.Error($"[DRIVER_PKG_CLIENT] SHA-256 mismatch!");
                                return new DriverDownloadResult
                                {
                                    Success = false,
                                    ErrorMessage = $"SHA-256 mismatch: expected {expectedPackageId}, got {actualHash}"
                                };
                            }
                            AppLogger.Log($"[DRIVER_PKG_CLIENT] SHA-256 OK. Extracting package...");

                            // H8: Cancel-during-verify (between verify and extract)
                            cancellationToken.ThrowIfCancellationRequested();

                            // Success — extract zip to final cache directory using safe extractor (H2)
                            string finalDir = Path.Combine(_cacheRoot, expectedPackageId);
                            if (Directory.Exists(finalDir))
                                Directory.Delete(finalDir, true);

                            try
                            {
                                using (var zipStream = new MemoryStream(allBytes))
                                using (var archive = new ZipArchive(zipStream, ZipArchiveMode.Read))
                                {
                                    SafeZipExtractor.ExtractSafely(archive, finalDir);
                                }
                            }
                            catch
                            {
                                // Clean up partial extraction on failure
                                try { if (Directory.Exists(finalDir)) Directory.Delete(finalDir, true); } catch { }
                                throw;
                            }

                            // H3: Write verification marker after successful extract
                            var marker = new DriverPackageVerifiedMarker
                            {
                                Sha256 = actualHash,
                                TotalSizeBytes = allBytes.Length,
                                FileCount = Directory.GetFiles(finalDir, "*", SearchOption.AllDirectories).Length,
                                ExtractedAtUtc = DateTime.UtcNow
                            };
                            string markerJson = JsonSerializer.Serialize(marker, new JsonSerializerOptions { WriteIndented = true });
                            File.WriteAllText(Path.Combine(finalDir, ".verified.json"), markerJson);

                            progress?.Report(1.0);
                            AppLogger.Log($"[DRIVER_PKG_CLIENT] Driver package downloaded, verified, and extracted: {allBytes.Length} bytes zip, SHA-256={actualHash[..16]}...");

                            return new DriverDownloadResult
                            {
                                Success = true,
                                PackageDirectory = finalDir,
                                FromCache = false
                            };
                        }
                        else if (packetType == Constants.PacketTypeDriverPackageError)
                        {
                            var error = JsonSerializer.Deserialize<DriverPackageError>(
                                Encoding.UTF8.GetString(payload));
                            return new DriverDownloadResult
                            {
                                Success = false,
                                ErrorMessage = error?.Message ?? "Unknown server error."
                            };
                        }
                        else
                        {
                            return new DriverDownloadResult
                            {
                                Success = false,
                                ErrorMessage = $"Unexpected packet type: 0x{packetType:X2}"
                            };
                        }
                    }
                }
            }
            catch (TimeoutException ex)
            {
                // H1: Per-read timeout — propagate so caller can show Cancel/Retry dialog
                AppLogger.Error("[DRIVER_PKG_CLIENT] Transfer timed out", ex);
                return new DriverDownloadResult
                {
                    Success = false,
                    ErrorMessage = "Driver transfer timed out — server stalled.",
                    TimedOut = true
                };
            }
            catch (OperationCanceledException)
            {
                // User cancellation
                return new DriverDownloadResult
                {
                    Success = false,
                    ErrorMessage = "Download cancelled."
                };
            }
            catch (Exception ex)
            {
                AppLogger.Error("[DRIVER_PKG_CLIENT] Download failed", ex);
                return new DriverDownloadResult
                {
                    Success = false,
                    ErrorMessage = $"Download failed: {ex.Message}"
                };
            }
            finally
            {
                // Clean up temp directory on failure
                try { if (Directory.Exists(tempDir)) Directory.Delete(tempDir, true); } catch { }
            }
        }

        /// <summary>
        /// Evicts old cache entries to keep total cache size under the LRU cap.
        /// </summary>
        public void EnforceCacheLimit()
        {
            try
            {
                if (!Directory.Exists(_cacheRoot)) return;

                var dirs = Directory.GetDirectories(_cacheRoot)
                    .Where(d => !Path.GetFileName(d).StartsWith("tmp_"))
                    .Select(d => new DirectoryInfo(d))
                    .OrderByDescending(d => d.LastAccessTimeUtc)
                    .ToList();

                long totalSize = dirs.Sum(d => d.EnumerateFiles("*", SearchOption.AllDirectories).Sum(f => f.Length));

                // Evict oldest entries if over limit
                while (totalSize > Constants.ClientDriverCacheMaxBytes && dirs.Count > 1)
                {
                    var oldest = dirs.Last();
                    long dirSize = oldest.EnumerateFiles("*", SearchOption.AllDirectories).Sum(f => f.Length);
                    try
                    {
                        oldest.Delete(true);
                        totalSize -= dirSize;
                        dirs.RemoveAt(dirs.Count - 1);
                        AppLogger.Log($"[DRIVER_PKG_CLIENT] Evicted cache entry: {oldest.Name}");
                    }
                    catch
                    {
                        break; // Can't delete, stop evicting
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("[DRIVER_PKG_CLIENT] Cache eviction error", ex);
            }
        }

        /// <summary>
        /// H4: Deterministic .inf selection. Priority: manifest.InfName → sole .inf → fail.
        /// Returns null if no .inf found or ambiguous (multiple .inf, no manifest).
        /// </summary>
        public string? ResolveInfPath(string packageDirectory, string? manifestInfName = null)
        {
            // Windows built-in INF files that pnputil may co-export as dependencies.
            // These must NEVER be used as the driver INF — they are OS infrastructure.
            var windowsSystemInfs = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
            {
                "ntprint.inf",
                "prnms001.inf", "prnms002.inf", "prnms003.inf", "prnms004.inf",
                "prnms005.inf", "prnms006.inf", "prnms007.inf", "prnms008.inf",
                "prnms009.inf", "prnms010.inf", "prnms011.inf", "prnms012.inf",
                "usbprint.inf", "wsdprint.inf", "wsprint.inf",
                "printqueue.inf", "prnroot.inf",
                "hpbusenum.inf", "hpvirtualbus.inf",
            };

            try
            {
                var allInfFiles = Directory.GetFiles(packageDirectory, "*.inf", SearchOption.AllDirectories);
                AppLogger.Log($"[DRIVER_PKG_CLIENT] ResolveInfPath: found {allInfFiles.Length} .inf file(s): {string.Join(", ", allInfFiles.Select(Path.GetFileName))}");

                if (allInfFiles.Length == 0)
                {
                    AppLogger.Error("[DRIVER_PKG_CLIENT] ResolveInfPath: no .inf files found in package directory.");
                    return null;
                }

                // Priority 1: manifest InfName (exact match — most reliable)
                if (!string.IsNullOrWhiteSpace(manifestInfName))
                {
                    var manifestMatch = allInfFiles.FirstOrDefault(f =>
                        string.Equals(Path.GetFileName(f), manifestInfName, StringComparison.OrdinalIgnoreCase));
                    if (manifestMatch != null)
                    {
                        AppLogger.Log($"[DRIVER_PKG_CLIENT] ResolveInfPath: Priority 1 match via manifest InfName → {Path.GetFileName(manifestMatch)}");
                        return manifestMatch;
                    }
                    AppLogger.Log($"[DRIVER_PKG_CLIENT] ResolveInfPath: manifest InfName '{manifestInfName}' not found on disk, continuing.");
                }

                // Priority 2: filter out Windows system INFs, use remaining
                var driverInfs = allInfFiles
                    .Where(f => !windowsSystemInfs.Contains(Path.GetFileName(f)))
                    .ToArray();

                AppLogger.Log($"[DRIVER_PKG_CLIENT] ResolveInfPath: after filtering system INFs → {driverInfs.Length} candidate(s): {string.Join(", ", driverInfs.Select(Path.GetFileName))}");

                if (driverInfs.Length == 1)
                {
                    AppLogger.Log($"[DRIVER_PKG_CLIENT] ResolveInfPath: Priority 2 single candidate → {Path.GetFileName(driverInfs[0])}");
                    return driverInfs[0];
                }

                if (driverInfs.Length > 1)
                {
                    // Priority 3: prefer oem*.inf (pnputil export naming convention)
                    var oemInf = driverInfs.FirstOrDefault(f =>
                        Path.GetFileName(f).StartsWith("oem", StringComparison.OrdinalIgnoreCase));

                    // Priority 3b: prefer architecture-neutral INF over arch-specific
                    // e.g., prefer "CNMC3280ZK.inf" over "CNMC3280ZK_x64.inf"
                    var archSuffixes = new[] { "_x64", "_x86", "_arm64", "_arm", "_ia64", "64", "86" };
                    var neutralInf = driverInfs.FirstOrDefault(f =>
                        !archSuffixes.Any(s => Path.GetFileNameWithoutExtension(f)
                            .EndsWith(s, StringComparison.OrdinalIgnoreCase)));

                    var best = neutralInf ?? oemInf ?? driverInfs[0];
                    AppLogger.Log($"[DRIVER_PKG_CLIENT] ResolveInfPath: Priority 3 from {driverInfs.Length} candidates → {Path.GetFileName(best)}");
                    return best;
                }

                // All files were system INFs — last resort: use them
                if (allInfFiles.Length == 1)
                {
                    AppLogger.Log($"[DRIVER_PKG_CLIENT] ResolveInfPath: only system INF found, using as last resort → {Path.GetFileName(allInfFiles[0])}");
                    return allInfFiles[0];
                }

                AppLogger.Error($"[DRIVER_PKG_CLIENT] ResolveInfPath: ambiguous — all .inf files are system INFs: {string.Join(", ", allInfFiles.Select(Path.GetFileName))}");
                return null;
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[DRIVER_PKG_CLIENT] ResolveInfPath: exception — {ex.Message}");
                return null;
            }
        }


        /// <summary>
        /// Legacy .inf selection (deprecated — use ResolveInfPath instead).
        /// </summary>
        [Obsolete("Use ResolveInfPath(string, string?) for deterministic selection.")]
        public string? GetDriverInfPath(string packageDirectory)
        {
            try
            {
                var infFiles = Directory.GetFiles(packageDirectory, "*.inf", SearchOption.AllDirectories);
                if (infFiles.Length > 0)
                    return infFiles[0];

                return null;
            }
            catch
            {
                return null;
            }
        }

        /// <summary>
        /// H1: Read exactly 4 bytes with per-read timeout.
        /// Uses a linked CancellationTokenSource with CancelAfter to detect stalls.
        /// </summary>
        internal static async Task<int> ReadInt32Async(NetworkStream stream, CancellationToken ct)
        {
            byte[] buf = new byte[4];
            int read = 0;
            while (read < 4)
            {
                using var readCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
                readCts.CancelAfter(TimeSpan.FromMilliseconds(Constants.DriverTransferReadTimeoutMs));
                try
                {
                    int n = await stream.ReadAsync(buf.AsMemory(read, 4 - read), readCts.Token);
                    if (n == 0) throw new EndOfStreamException();
                    read += n;
                }
                catch (OperationCanceledException) when (!ct.IsCancellationRequested)
                {
                    // Per-read timeout (not user cancel)
                    throw new TimeoutException("Driver transfer timed out — server stalled.");
                }
            }
            return BitConverter.ToInt32(buf, 0);
        }

        /// <summary>
        /// H1: Read exactly 'count' bytes with per-read timeout.
        /// Uses a linked CancellationTokenSource with CancelAfter to detect stalls.
        /// </summary>
        internal static async Task<byte[]> ReadBytesAsync(NetworkStream stream, int count, CancellationToken ct)
        {
            byte[] buf = new byte[count];
            int read = 0;
            while (read < count)
            {
                using var readCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
                readCts.CancelAfter(TimeSpan.FromMilliseconds(Constants.DriverTransferReadTimeoutMs));
                try
                {
                    int n = await stream.ReadAsync(buf.AsMemory(read, count - read), readCts.Token);
                    if (n == 0) throw new EndOfStreamException();
                    read += n;
                }
                catch (OperationCanceledException) when (!ct.IsCancellationRequested)
                {
                    // Per-read timeout (not user cancel)
                    throw new TimeoutException("Driver transfer timed out — server stalled.");
                }
            }
            return buf;
        }
    }

    public class DriverDownloadResult
    {
        public bool Success { get; set; }
        public string? PackageDirectory { get; set; }
        public bool FromCache { get; set; }
        public string? ErrorMessage { get; set; }

        /// <summary>
        /// H1: Indicates the download failed due to a per-read timeout (server stall).
        /// Used by the caller to show Cancel/Retry dialog.
        /// </summary>
        public bool TimedOut { get; set; }
    }

    /// <summary>
    /// H3: Verification marker written to cache after successful download + extract.
    /// </summary>
    public class DriverPackageVerifiedMarker
    {
        public string Sha256 { get; set; } = string.Empty;
        public long TotalSizeBytes { get; set; }
        public int FileCount { get; set; }
        public DateTime ExtractedAtUtc { get; set; }
    }
}
