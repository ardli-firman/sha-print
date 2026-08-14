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
            // Check local cache first
            string cachedDir = Path.Combine(_cacheRoot, expectedPackageId);
            if (Directory.Exists(cachedDir))
            {
                string manifestPath = Path.Combine(cachedDir, "manifest.json");
                if (File.Exists(manifestPath))
                {
                    AppLogger.Log($"[DRIVER_PKG_CLIENT] Using cached driver package: {expectedPackageId[..16]}...");
                    progress?.Report(1.0);
                    return new DriverDownloadResult
                    {
                        Success = true,
                        PackageDirectory = cachedDir,
                        FromCache = true
                    };
                }
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
                var connectTask = client.ConnectAsync(serverIp, Constants.PrintTcpPort);
                if (await Task.WhenAny(connectTask, Task.Delay(Constants.DriverPackageTransferTimeoutMs, cancellationToken)) != connectTask)
                {
                    return new DriverDownloadResult { Success = false, ErrorMessage = "Connection timeout." };
                }
                await connectTask;

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
                            progress?.Report((double)receivedChunks / totalChunks);
                        }
                        else if (packetType == Constants.PacketTypeDriverPackageComplete)
                        {
                            var complete = JsonSerializer.Deserialize<DriverPackageComplete>(
                                Encoding.UTF8.GetString(payload));
                            if (complete == null)
                            {
                                return new DriverDownloadResult { Success = false, ErrorMessage = "Invalid completion message." };
                            }

                            allBytes = ms.ToArray();

                            // Verify total size
                            if (allBytes.Length != expectedSize)
                            {
                                return new DriverDownloadResult
                                {
                                    Success = false,
                                    ErrorMessage = $"Size mismatch: expected {expectedSize}, got {allBytes.Length}"
                                };
                            }

                            // Verify SHA-256
                            string actualHash = Convert.ToHexString(SHA256.HashData(allBytes)).ToLowerInvariant();
                            if (!actualHash.Equals(expectedPackageId, StringComparison.OrdinalIgnoreCase))
                            {
                                return new DriverDownloadResult
                                {
                                    Success = false,
                                    ErrorMessage = $"SHA-256 mismatch: expected {expectedPackageId}, got {actualHash}"
                                };
                            }

                            // Success — extract zip to final cache directory
                            string finalDir = Path.Combine(_cacheRoot, expectedPackageId);
                            if (Directory.Exists(finalDir))
                                Directory.Delete(finalDir, true);

                            // Extract zip archive to finalDir
                            using (var zipStream = new MemoryStream(allBytes))
                            using (var archive = new ZipArchive(zipStream, ZipArchiveMode.Read))
                            {
                                archive.ExtractToDirectory(finalDir);
                            }

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
        /// Extracts .inf and other driver files from the cached package.dat
        /// into a directory suitable for pnputil /add-driver or Add-PrinterDriver -InfPath.
        /// Returns the path to the directory containing the .inf file, or null on failure.
        /// </summary>
        public string? GetDriverInfPath(string packageDirectory)
        {
            try
            {
                // Look for .inf files in the package directory
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

        private static async Task<int> ReadInt32Async(NetworkStream stream, CancellationToken ct)
        {
            byte[] buf = new byte[4];
            int read = 0;
            while (read < 4)
            {
                int n = await stream.ReadAsync(buf.AsMemory(read, 4 - read), ct);
                if (n == 0) throw new EndOfStreamException();
                read += n;
            }
            return BitConverter.ToInt32(buf, 0);
        }

        private static async Task<byte[]> ReadBytesAsync(NetworkStream stream, int count, CancellationToken ct)
        {
            byte[] buf = new byte[count];
            int read = 0;
            while (read < count)
            {
                int n = await stream.ReadAsync(buf.AsMemory(read, count - read), ct);
                if (n == 0) throw new EndOfStreamException();
                read += n;
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
    }
}
