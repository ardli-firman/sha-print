using System;
using System.Buffers.Binary;
using System.Collections.Concurrent;
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
    [Obsolete("Deprecated: IPP server handles all printing. Driver download no longer needed.")]
    public class DriverPackageManager
    {
        private readonly string _cacheRoot;
        private static readonly ConcurrentDictionary<string, SemaphoreSlim> _downloadLocks = new(StringComparer.OrdinalIgnoreCase);
        private static readonly ConcurrentDictionary<string, int> _activeDownloads = new(StringComparer.OrdinalIgnoreCase);

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
            if (!DriverPackageIdValidator.IsValid(expectedPackageId))
            {
                return new DriverDownloadResult
                {
                    Success = false,
                    ErrorMessage = "Driver package identifier must be exactly 64 hexadecimal characters."
                };
            }

            // H5: Size-cap pre-check
            if (expectedSize <= 0 || expectedSize > Constants.MaxDriverPackageSize)
            {
                return new DriverDownloadResult
                {
                    Success = false,
                    ErrorMessage = $"Driver package size ({expectedSize:N0} bytes) exceeds limit ({Constants.MaxDriverPackageSize / (1024 * 1024)} MB)."
                };
            }

            var downloadLock = _downloadLocks.GetOrAdd(expectedPackageId, _ => new SemaphoreSlim(1, 1));
            if (!await downloadLock.WaitAsync(TimeSpan.FromSeconds(10), cancellationToken).ConfigureAwait(false))
            {
                return new DriverDownloadResult { Success = false, ErrorMessage = "Download already in progress for this package." };
            }

            try
            {
                _activeDownloads.AddOrUpdate(expectedPackageId, 1, (_, count) => count + 1);
                // H3: cache check occurs under the package lock, so two clients
                // cannot delete an active extraction or accept a partial marker.
                string cachedDir = Path.Combine(_cacheRoot, expectedPackageId);
                if (TryGetVerifiedCache(cachedDir, expectedPackageId, expectedSize, out _))
                {
                    AppLogger.Log($"[DRIVER_PKG_CLIENT] Cache verified: {expectedPackageId[..16]}...");
                    progress?.Report(1.0);
                    return new DriverDownloadResult { Success = true, PackageDirectory = cachedDir, FromCache = true };
                }
                // Keep an existing verified/partial directory until a fully
                // verified replacement is ready. This preserves the last usable
                // package if the new transfer is cancelled.
                return await DownloadFromServerAsync(serverIp, printerName, expectedPackageId, expectedSize, progress, cancellationToken);
            }
            finally
            {
                if (_activeDownloads.TryGetValue(expectedPackageId, out int activeCount)
                    && activeCount <= 1)
                    _activeDownloads.TryRemove(expectedPackageId, out _);
                else
                    _activeDownloads.AddOrUpdate(expectedPackageId, 0, (_, count) => Math.Max(0, count - 1));
                downloadLock.Release();
            }
        }

        private async Task<DriverDownloadResult> DownloadFromServerAsync(
            string serverIp,
            string printerName,
            string expectedPackageId,
            long expectedSize,
            IProgress<double>? progress,
            CancellationToken cancellationToken,
            TimeSpan? transferTimeout = null)
        {
            string tempDir = Path.Combine(_cacheRoot, $"tmp_{Guid.NewGuid():N}");
            string tempZipPath = Path.Combine(tempDir, "package.zip");
            Directory.CreateDirectory(tempDir);

            try
            {
                using var client = new TcpClient();
                AppLogger.Log($"[DRIVER_PKG_CLIENT] Connecting to {serverIp}:{Constants.PrintTcpPort}...");
                var connectTask = client.ConnectAsync(serverIp, Constants.PrintTcpPort);
                using var transferDeadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
                transferDeadline.CancelAfter(transferTimeout ?? TimeSpan.FromMilliseconds(Constants.DriverPackageTransferTimeoutMs));
                if (await Task.WhenAny(connectTask, Task.Delay(Timeout.InfiniteTimeSpan, transferDeadline.Token)).ConfigureAwait(false) != connectTask)
                {
                    transferDeadline.Token.ThrowIfCancellationRequested();
                    return new DriverDownloadResult { Success = false, ErrorMessage = "Connection timeout.", TimedOut = true };
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
                byte[] requestPacket;
                try
                {
                    requestPacket = BuildDriverPackageRequestPacket(request);
                }
                catch (InvalidDataException)
                {
                    return new DriverDownloadResult
                    {
                        Success = false,
                        ErrorMessage = "Driver package request is too large."
                    };
                }

                // Write one bounded packet with cancellation-aware async I/O. The
                // receiver expects little-endian type and length prefixes.
                try
                {
                    await stream.WriteAsync(requestPacket.AsMemory(), transferDeadline.Token).ConfigureAwait(false);
                    await stream.FlushAsync(transferDeadline.Token).ConfigureAwait(false);
                }
                finally
                {
                    CryptographicOperations.ZeroMemory(requestPacket);
                }

                // Receive and hash chunks directly into a bounded temporary file.
                DriverPackageComplete? completeMessage = null;
                int totalChunks = 0;
                long receivedBytes = 0;
                using (var packageFile = new FileStream(tempZipPath, FileMode.CreateNew, FileAccess.Write, FileShare.None, 64 * 1024, FileOptions.Asynchronous | FileOptions.SequentialScan))
                using (var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256))
                {
                    int receivedChunks = 0;
                    DriverChunkSequence? sequence = null;

                    while (true)
                    {
                        transferDeadline.Token.ThrowIfCancellationRequested();

                        var packetType = await ReadInt32Async(stream, transferDeadline.Token);
                        var payloadLength = await ReadInt32Async(stream, transferDeadline.Token);

                        if (payloadLength <= 0 || payloadLength > Constants.DriverPackageChunkSize * 2 + 16 * 1024)
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

                        byte[] payload = await ReadBytesAsync(stream, payloadLength, transferDeadline.Token);

                        try
                        {
                        if (packetType == Constants.PacketTypeDriverPackageChunk)
                        {
                            var chunk = JsonSerializer.Deserialize<DriverPackageChunk>(
                                Encoding.UTF8.GetString(payload));
                            if (chunk == null)
                            {
                                return new DriverDownloadResult { Success = false, ErrorMessage = "Invalid chunk data." };
                            }

                            if (chunk.TotalChunks <= 0 || chunk.TotalChunks > (Constants.MaxDriverPackageSize + Constants.DriverPackageChunkSize - 1) / Constants.DriverPackageChunkSize)
                            {
                                CryptographicOperations.ZeroMemory(payload);
                                return new DriverDownloadResult { Success = false, ErrorMessage = "Invalid total chunk count." };
                            }
                            sequence ??= new DriverChunkSequence(chunk.TotalChunks);
                            if (!sequence.TryAccept(chunk.ChunkIndex, chunk.TotalChunks, out string sequenceError))
                            {
                                CryptographicOperations.ZeroMemory(chunk.Data);
                                CryptographicOperations.ZeroMemory(payload);
                                return new DriverDownloadResult { Success = false, ErrorMessage = sequenceError };
                            }

                            byte[] rawChunk = Array.Empty<byte>();
                            try
                            {
                                // Decrypt the chunk
                                rawChunk = CryptoHelper.DecryptAesGcm(chunk.Data);

                                if (rawChunk.Length <= 0 || rawChunk.Length > Constants.DriverPackageChunkSize || receivedBytes > Constants.MaxDriverPackageSize - rawChunk.Length)
                                    return new DriverDownloadResult { Success = false, ErrorMessage = "Invalid driver package chunk size." };

                                // Verify per-chunk HMAC
                                if (!CryptoHelper.VerifyHmac(rawChunk, chunk.ChunkHmac))
                                {
                                    return new DriverDownloadResult
                                    {
                                        Success = false,
                                        ErrorMessage = $"Chunk {chunk.ChunkIndex} HMAC verification failed."
                                    };
                                }

                                await packageFile.WriteAsync(rawChunk, transferDeadline.Token).ConfigureAwait(false);
                                hash.AppendData(rawChunk);
                                receivedBytes += rawChunk.Length;
                                receivedChunks++;
                                totalChunks = chunk.TotalChunks;
                                AppLogger.Log($"[DRIVER_PKG_CLIENT] Chunk {chunk.ChunkIndex + 1}/{totalChunks} received ({rawChunk.Length:N0} bytes).");
                                progress?.Report((double)receivedChunks / totalChunks);
                            }
                            finally
                            {
                                if (rawChunk.Length > 0)
                                    CryptographicOperations.ZeroMemory(rawChunk);
                                if (chunk.Data.Length > 0)
                                    CryptographicOperations.ZeroMemory(chunk.Data);
                            }
                        }
                        else if (packetType == Constants.PacketTypeDriverPackageComplete)
                        {
                            completeMessage = JsonSerializer.Deserialize<DriverPackageComplete>(
                                Encoding.UTF8.GetString(payload));
                            if (completeMessage == null)
                            {
                                return new DriverDownloadResult { Success = false, ErrorMessage = "Invalid completion message." };
                            }

                            if (sequence == null || !sequence.IsComplete || completeMessage.TotalChunks != totalChunks)
                                return new DriverDownloadResult { Success = false, ErrorMessage = "Driver package has missing chunks." };
                            await packageFile.FlushAsync(transferDeadline.Token).ConfigureAwait(false);
                            AppLogger.Log($"[DRIVER_PKG_CLIENT] All chunks received: {receivedBytes:N0} bytes total (expected={expectedSize:N0}).");

                            // Keep the hard bound immediately before hashing and
                            // publication as a second line of defence against a
                            // future framing/metadata regression.
                            if (receivedBytes <= 0 || receivedBytes > Constants.MaxDriverPackageSize)
                            {
                                return new DriverDownloadResult
                                {
                                    Success = false,
                                    ErrorMessage = "Driver package exceeds the configured size limit."
                                };
                            }
                            string actualHash = Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant();

                            // Verify total size — skip if expectedSize is 0 (stale discovery metadata)
                            if (expectedSize > 0 && receivedBytes != expectedSize)
                            {
                                AppLogger.Error($"[DRIVER_PKG_CLIENT] Size mismatch: expected {expectedSize:N0}, got {receivedBytes:N0}");
                                return new DriverDownloadResult
                                {
                                    Success = false,
                                    ErrorMessage = $"Size mismatch: expected {expectedSize}, got {receivedBytes}"
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
                            transferDeadline.Token.ThrowIfCancellationRequested();

                            // Success — extract zip to final cache directory using safe extractor (H2)
                            string finalDir = Path.Combine(_cacheRoot, expectedPackageId);
                            string extractionDir = Path.Combine(tempDir, "extracted");

                            try
                            {
                                using (var packageRead = new FileStream(tempZipPath, FileMode.Open, FileAccess.Read, FileShare.Read, 64 * 1024, FileOptions.SequentialScan))
                                using (var archive = new ZipArchive(packageRead, ZipArchiveMode.Read))
                                {
                                    SafeZipExtractor.ExtractSafely(archive, extractionDir);
                                }

                                // A verified cache entry wins over a concurrent or
                                // externally-triggered re-download. Only remove an
                                // incomplete destination after the new extraction is
                                // complete and cancellation has been checked.
                                transferDeadline.Token.ThrowIfCancellationRequested();
                                if (TryGetVerifiedCache(finalDir, expectedPackageId, expectedSize, out _))
                                {
                                    TryDeleteDirectory(extractionDir);
                                }
                                else
                                {
                                    var marker = new DriverPackageVerifiedMarker
                                    {
                                        Sha256 = actualHash,
                                        TotalSizeBytes = receivedBytes,
                                        FileCount = Directory.GetFiles(extractionDir, "*", SearchOption.AllDirectories).Length,
                                        ExtractedAtUtc = DateTime.UtcNow
                                    };
                                    string markerJson = JsonSerializer.Serialize(marker, new JsonSerializerOptions { WriteIndented = true });
                                    File.WriteAllText(Path.Combine(extractionDir, ".verified.json"), markerJson);
                                    transferDeadline.Token.ThrowIfCancellationRequested();

                                    // Move is atomic within the cache root. A partial
                                    // destination is safe to remove now because all
                                    // bytes, extraction, and verification completed in
                                    // the isolated directory.
                                    TryDeleteDirectory(finalDir);
                                    Directory.Move(extractionDir, finalDir);
                                }
                            }
                            catch
                            {
                                // Clean up partial extraction on failure
                                TryDeleteDirectory(extractionDir);
                                throw;
                            }

                            progress?.Report(1.0);
                            AppLogger.Log($"[DRIVER_PKG_CLIENT] Driver package downloaded, verified, and extracted: {receivedBytes} bytes zip, SHA-256={actualHash[..16]}...");

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
                        finally
                        {
                            CryptographicOperations.ZeroMemory(payload);
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
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                // User cancellation
                return CreateCancellationResult(userCancelled: true);
            }
            catch (OperationCanceledException)
            {
                AppLogger.Error("[DRIVER_PKG_CLIENT] Transfer deadline elapsed.");
                return CreateCancellationResult(userCancelled: false);
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
                // Temporary archive/extraction is never retained after success or failure.
                TryDeleteDirectory(tempDir);
            }
        }

        internal static bool TryGetVerifiedCache(string directory, string packageId, long expectedSize, out DriverPackageVerifiedMarker? marker)
        {
            marker = null;
            string markerPath = Path.Combine(directory, ".verified.json");
            if (!Directory.Exists(directory) || !File.Exists(markerPath))
                return false;
            try
            {
                marker = JsonSerializer.Deserialize<DriverPackageVerifiedMarker>(File.ReadAllText(markerPath));
                return marker != null
                    && DriverPackageIdValidator.IsValid(marker.Sha256)
                    && marker.Sha256.Equals(packageId, StringComparison.OrdinalIgnoreCase)
                    && marker.TotalSizeBytes == expectedSize;
            }
            catch { return false; }
        }

        internal static byte[] BuildDriverPackageRequestPacket(DriverPackageRequest request)
        {
            byte[] requestJson = JsonSerializer.SerializeToUtf8Bytes(request);
            try
            {
                if (requestJson.Length > 16 * 1024)
                    throw new InvalidDataException("Driver package request is too large.");

                byte[] packet = new byte[sizeof(int) + sizeof(int) + requestJson.Length];
                BinaryPrimitives.WriteInt32LittleEndian(packet.AsSpan(0, sizeof(int)), Constants.PacketTypeDriverPackageRequest);
                BinaryPrimitives.WriteInt32LittleEndian(packet.AsSpan(sizeof(int), sizeof(int)), requestJson.Length);
                requestJson.CopyTo(packet.AsSpan(sizeof(int) * 2));
                return packet;
            }
            finally
            {
                CryptographicOperations.ZeroMemory(requestJson);
            }
        }

        internal static DriverDownloadResult CreateCancellationResult(bool userCancelled)
            => userCancelled
                ? new DriverDownloadResult { Success = false, ErrorMessage = "Download cancelled." }
                : new DriverDownloadResult
                {
                    Success = false,
                    ErrorMessage = "Driver transfer timed out — server stalled.",
                    TimedOut = true
                };

        internal Task<DriverDownloadResult> DownloadDriverPackageForTestAsync(
            string serverIp,
            string printerName,
            string expectedPackageId,
            long expectedSize,
            TimeSpan transferTimeout,
            CancellationToken cancellationToken = default)
            => DownloadFromServerAsync(
                serverIp,
                printerName,
                expectedPackageId,
                expectedSize,
                progress: null,
                cancellationToken: cancellationToken,
                transferTimeout: transferTimeout);

        private static void TryDeleteDirectory(string directory)
        {
            try { if (Directory.Exists(directory)) Directory.Delete(directory, true); }
            catch { /* cleanup is best effort; never mask the transfer result */ }
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
                    .Where(d => !Path.GetFileName(d).StartsWith("tmp_")
                        && DriverPackageIdValidator.IsValid(Path.GetFileName(d))
                        && !_activeDownloads.ContainsKey(Path.GetFileName(d)))
                    .Select(d => new DirectoryInfo(d))
                    .OrderByDescending(d => d.LastAccessTimeUtc)
                    .ToList();

                long totalSize = dirs.Sum(d => d.EnumerateFiles("*", SearchOption.AllDirectories).Sum(f => f.Length));

                // Evict oldest entries if over limit
                while (totalSize > Constants.ClientDriverCacheMaxBytes && dirs.Count > 1)
                {
                    var oldest = dirs.Last();
                    long dirSize = oldest.EnumerateFiles("*", SearchOption.AllDirectories).Sum(f => f.Length);
                    string packageId = oldest.Name;
                    var packageLock = _downloadLocks.GetOrAdd(packageId, _ => new SemaphoreSlim(1, 1));
                    if (!packageLock.Wait(0))
                    {
                        // A transfer owns the package lock (or has already marked
                        // itself active); never delete its directory. Remove this
                        // candidate from this pass and let a later pass retry it.
                        dirs.RemoveAt(dirs.Count - 1);
                        continue;
                    }
                    if (_activeDownloads.ContainsKey(packageId))
                    {
                        packageLock.Release();
                        dirs.RemoveAt(dirs.Count - 1);
                        continue;
                    }
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
                    finally
                    {
                        packageLock.Release();
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
