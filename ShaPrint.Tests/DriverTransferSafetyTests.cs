using System;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Abstractions;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.Services.Client;
using Xunit;

namespace ShaPrint.Tests
{
    public sealed class DriverTransferSafetyTests
    {
        [Theory]
        [InlineData("")]
        [InlineData("short")]
        [InlineData("000000000000000000000000000000000000000000000000000000000000000g")]
        [InlineData("000000000000000000000000000000000000000000000000000000000000000 ")]
        public void PackageIdValidator_RejectsMalformedIds(string id)
        {
            Assert.False(DriverPackageIdValidator.IsValid(id));
        }

        [Fact]
        public void PackageIdValidator_AcceptsExactly64HexCharacters()
        {
            Assert.True(DriverPackageIdValidator.IsValid(new string('a', 64)));
            Assert.True(DriverPackageIdValidator.IsValid("0123456789abcdef0123456789ABCDEF0123456789abcdef0123456789ABCDEF"));
            Assert.False(DriverPackageIdValidator.IsValid(new string('a', 63)));
            Assert.False(DriverPackageIdValidator.IsValid(new string('a', 65)));
        }

        [Fact]
        public void DriverPackageRequestPacket_IsBoundedAndLittleEndian()
        {
            string packageId = new string('a', 64);
            byte[] packet = DriverPackageManager.BuildDriverPackageRequestPacket(new DriverPackageRequest
            {
                PrinterName = "Office Printer",
                DriverPackageId = packageId
            });

            int jsonLength = BitConverter.ToInt32(packet, sizeof(int));
            Assert.Equal(Constants.PacketTypeDriverPackageRequest, BitConverter.ToInt32(packet, 0));
            Assert.Equal(sizeof(int) * 2 + jsonLength, packet.Length);
            using var json = JsonDocument.Parse(packet.AsMemory(sizeof(int) * 2, jsonLength));
            Assert.Equal(packageId, json.RootElement.GetProperty("DriverPackageId").GetString());
            Assert.Throws<InvalidDataException>(() => DriverPackageManager.BuildDriverPackageRequestPacket(
                new DriverPackageRequest { PrinterName = new string('x', 20_000), DriverPackageId = packageId }));
            CryptographicOperations.ZeroMemory(packet);
        }

        [Fact]
        public void CancellationClassification_DistinguishesUserAndDeadline()
        {
            var user = DriverPackageManager.CreateCancellationResult(userCancelled: true);
            var deadline = DriverPackageManager.CreateCancellationResult(userCancelled: false);

            Assert.False(user.TimedOut);
            Assert.Contains("cancel", user.ErrorMessage!, StringComparison.OrdinalIgnoreCase);
            Assert.True(deadline.TimedOut);
            Assert.Contains("timed out", deadline.ErrorMessage!, StringComparison.OrdinalIgnoreCase);
        }

        [Fact]
        public void VerifiedCache_RejectsPartialDirectoryAndAcceptsExactMarker()
        {
            string root = Path.Combine(Path.GetTempPath(), "ShaPrint-driver-cache-" + Guid.NewGuid().ToString("N"));
            string packageId;
            byte[] package = Encoding.UTF8.GetBytes("verified package");
            try
            {
                packageId = Convert.ToHexString(SHA256.HashData(package)).ToLowerInvariant();
                Directory.CreateDirectory(root);
                File.WriteAllBytes(Path.Combine(root, "package.zip"), package);
                var marker = new DriverPackageVerifiedMarker
                {
                    Sha256 = packageId,
                    TotalSizeBytes = package.LongLength,
                    FileCount = 1,
                    ExtractedAtUtc = DateTime.UtcNow
                };
                File.WriteAllText(Path.Combine(root, ".verified.json"), JsonSerializer.Serialize(marker));

                Assert.True(DriverPackageManager.TryGetVerifiedCache(root, packageId, package.LongLength, out _));
                File.Delete(Path.Combine(root, ".verified.json"));
                Assert.False(DriverPackageManager.TryGetVerifiedCache(root, packageId, package.LongLength, out _));
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
                CryptographicOperations.ZeroMemory(package);
            }
        }

        [Fact]
        public async Task ExistingPackageHashCheck_RejectsSameLengthCorruption()
        {
            string root = Path.Combine(Path.GetTempPath(), "ShaPrint-driver-final-" + Guid.NewGuid().ToString("N"));
            byte[] package = Encoding.UTF8.GetBytes("verified package");
            string packageId = Convert.ToHexString(SHA256.HashData(package)).ToLowerInvariant();
            try
            {
                Directory.CreateDirectory(root);
                File.WriteAllBytes(Path.Combine(root, "package.zip"), package);
                File.WriteAllText(Path.Combine(root, "manifest.json"), JsonSerializer.Serialize(new DriverPackageManifest
                {
                    Sha256 = packageId,
                    TotalSizeBytes = package.LongLength
                }));

                var service = new ShaPrint.Server.DriverPackageService(
                    new MockProcessRunner(), new ShaPrint.Core.Abstractions.RealFileSystem());
                var method = typeof(ShaPrint.Server.DriverPackageService).GetMethod(
                    "IsCompleteFinalPackageAsync", BindingFlags.Instance | BindingFlags.NonPublic);
                Assert.NotNull(method);
                var validTask = (Task<bool>)method!.Invoke(service, new object[] { root, packageId, CancellationToken.None })!;
                Assert.True(await validTask);

                package[0] ^= 0x01;
                File.WriteAllBytes(Path.Combine(root, "package.zip"), package);
                var corruptTask = (Task<bool>)method.Invoke(service, new object[] { root, packageId, CancellationToken.None })!;
                Assert.False(await corruptTask);
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
                CryptographicOperations.ZeroMemory(package);
            }
        }

        [Fact]
        public void EnforceCacheLimit_SkipsActivePackageDirectory()
        {
            string root = Path.Combine(Path.GetTempPath(), "ShaPrint-driver-eviction-" + Guid.NewGuid().ToString("N"));
            string activeId = new string('b', 64);
            string otherId = new string('c', 64);
            var manager = new DriverPackageManager();
            var rootField = typeof(DriverPackageManager).GetField("_cacheRoot", BindingFlags.Instance | BindingFlags.NonPublic);
            var activeField = typeof(DriverPackageManager).GetField("_activeDownloads", BindingFlags.Static | BindingFlags.NonPublic);
            Assert.NotNull(rootField);
            Assert.NotNull(activeField);
            var activeDownloads = (ConcurrentDictionary<string, int>)activeField!.GetValue(null)!;
            try
            {
                rootField!.SetValue(manager, root);
                string activeDir = Path.Combine(root, activeId);
                string otherDir = Path.Combine(root, otherId);
                Directory.CreateDirectory(activeDir);
                Directory.CreateDirectory(otherDir);
                // SetLength creates a sparse/zero-filled file on the test volume,
                // so eviction observes a realistic over-cap cache without a
                // multi-hundred-megabyte in-memory allocation.
                using (var package = new FileStream(Path.Combine(activeDir, "package.zip"), FileMode.CreateNew, FileAccess.Write, FileShare.Read))
                    package.SetLength(Constants.ClientDriverCacheMaxBytes + 1);
                File.WriteAllBytes(Path.Combine(otherDir, "package.zip"), new byte[] { 1 });
                Directory.SetLastAccessTimeUtc(activeDir, DateTime.UtcNow.AddMinutes(-2));
                Directory.SetLastAccessTimeUtc(otherDir, DateTime.UtcNow);
                activeDownloads[activeId] = 1;

                manager.EnforceCacheLimit();

                Assert.True(Directory.Exists(activeDir));
                Assert.True(Directory.Exists(otherDir));
            }
            finally
            {
                activeDownloads.TryRemove(activeId, out _);
                try { Directory.Delete(root, recursive: true); } catch { }
            }
        }

        [Fact]
        public async Task DownloadTransferDeadline_CleansTempDirectoryWhenServerStalls()
        {
            string root = Path.Combine(Path.GetTempPath(), "ShaPrint-driver-stall-" + Guid.NewGuid().ToString("N"));
            string packageId = new string('d', 64);
            var manager = new DriverPackageManager();
            var rootField = typeof(DriverPackageManager).GetField("_cacheRoot", BindingFlags.Instance | BindingFlags.NonPublic);
            Assert.NotNull(rootField);
            rootField!.SetValue(manager, root);

            using var listener = new TcpListener(IPAddress.Loopback, Constants.PrintTcpPort);
            listener.Start();
            Task<TcpClient> accepted = listener.AcceptTcpClientAsync();
            try
            {
                Task<DriverDownloadResult> download = manager.DownloadDriverPackageForTestAsync(
                    "127.0.0.1", "Stalled printer", packageId, 1,
                    TimeSpan.FromMilliseconds(250));
                using TcpClient serverClient = await accepted;
                DriverDownloadResult result = await download;

                Assert.False(result.Success);
                Assert.True(result.TimedOut);
                Assert.True(Directory.Exists(root));
                Assert.Empty(Directory.GetDirectories(root, "tmp_*", SearchOption.TopDirectoryOnly));
            }
            finally
            {
                try { listener.Stop(); } catch { }
                try { if (Directory.Exists(root)) Directory.Delete(root, recursive: true); } catch { }
            }
        }

        [Fact]
        public async Task RealProcessRunner_TimeoutKillsChildProcessTree()
        {
            if (!OperatingSystem.IsWindows())
                return;

            string arguments = "-NoProfile -Command \"$child = Start-Process powershell.exe -ArgumentList '-NoProfile -Command Start-Sleep -Seconds 30' -PassThru; Write-Output $child.Id; Wait-Process -Id $child.Id\"";
            int childPid = 0;
            try
            {
                var result = await new RealProcessRunner().RunAsync(
                    "powershell.exe", arguments, TimeSpan.FromSeconds(1));
                Assert.False(result.Success);
                Assert.True(int.TryParse(result.Output.Trim(), out childPid), result.Output);
                Assert.True(WaitForProcessExit(childPid, TimeSpan.FromSeconds(2)));
            }
            finally
            {
                TryKillProcess(childPid);
            }
        }

        [Fact]
        public async Task DriverPackageService_SerializesConcurrentSameDriverExport()
        {
            var fs = new ConcurrentExportFileSystem();
            const string infPath = @"C:\Windows\INF\serialized-driver.inf";
            fs.AddFile(infPath, new byte[] { 1, 2, 3 });
            var runner = new ConcurrentExportProcessRunner(fs, infPath);
            var service = new ShaPrint.Server.DriverPackageService(runner, fs);

            Task<DriverPackageManifest?> first = service.GetDriverPackageAsync("Serialized Driver");
            Task<DriverPackageManifest?> second = service.GetDriverPackageAsync("Serialized Driver");
            DriverPackageManifest?[] results = await Task.WhenAll(first, second);

            Assert.All(results, result => Assert.NotNull(result));
            Assert.Equal(1, runner.ExportCalls);
            Assert.Equal(1, runner.MaximumConcurrentExports);
        }

        private static bool WaitForProcessExit(int processId, TimeSpan timeout)
        {
            if (processId <= 0)
                return false;
            try
            {
                using var process = Process.GetProcessById(processId);
                return process.HasExited || process.WaitForExit((int)timeout.TotalMilliseconds);
            }
            catch (ArgumentException)
            {
                return true;
            }
        }

        private static void TryKillProcess(int processId)
        {
            if (processId <= 0)
                return;
            try
            {
                using var process = Process.GetProcessById(processId);
                if (!process.HasExited)
                    process.Kill(entireProcessTree: true);
            }
            catch { }
        }

        private sealed class ConcurrentExportFileSystem : IFileSystem
        {
            private readonly ConcurrentDictionary<string, byte[]> _files = new(StringComparer.OrdinalIgnoreCase);
            private readonly ConcurrentDictionary<string, byte> _directories = new(StringComparer.OrdinalIgnoreCase);

            public void AddFile(string path, byte[] content)
            {
                _files[path] = content.ToArray();
                string? directory = Path.GetDirectoryName(path);
                if (directory != null)
                    _directories[directory] = 0;
            }

            public Task WriteAllBytesAsync(string path, byte[] data)
            {
                AddFile(path, data);
                return Task.CompletedTask;
            }

            public Task<byte[]> ReadAllBytesAsync(string path)
            {
                if (!_files.TryGetValue(path, out byte[]? content))
                    throw new FileNotFoundException(path);
                return Task.FromResult(content.ToArray());
            }

            public bool FileExists(string path) => _files.ContainsKey(path);
            public bool DirectoryExists(string path) => _directories.ContainsKey(path);
            public void CreateDirectory(string path) => _directories[path] = 0;
            public long GetFileSize(string path) => _files.TryGetValue(path, out byte[]? content) ? content.LongLength : 0;
            public void DeleteFile(string path) => _files.TryRemove(path, out _);

            public void DeleteDirectory(string path, bool recursive)
            {
                _directories.TryRemove(path, out _);
                if (!recursive)
                    return;
                string prefix = path.EndsWith(Path.DirectorySeparatorChar.ToString(), StringComparison.Ordinal)
                    ? path
                    : path + Path.DirectorySeparatorChar;
                foreach (string file in _files.Keys)
                {
                    if (file.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                        _files.TryRemove(file, out _);
                }
                foreach (string directory in _directories.Keys)
                {
                    if (directory.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                        _directories.TryRemove(directory, out _);
                }
            }

            public string[] GetFiles(string path, string searchPattern, SearchOption searchOption)
            {
                string prefix = path.EndsWith(Path.DirectorySeparatorChar.ToString(), StringComparison.Ordinal)
                    ? path
                    : path + Path.DirectorySeparatorChar;
                return _files.Keys
                    .Where(file => file.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                    .Where(file => searchOption == SearchOption.AllDirectories
                        || !file.Substring(prefix.Length).Contains(Path.DirectorySeparatorChar, StringComparison.Ordinal))
                    .ToArray();
            }
        }

        private sealed class ConcurrentExportProcessRunner : IProcessRunner
        {
            private readonly ConcurrentExportFileSystem _fileSystem;
            private readonly string _infPath;
            private int _activeExports;
            private int _exportCalls;
            private int _maximumConcurrentExports;

            public ConcurrentExportProcessRunner(ConcurrentExportFileSystem fileSystem, string infPath)
            {
                _fileSystem = fileSystem;
                _infPath = infPath;
            }

            public int ExportCalls => Volatile.Read(ref _exportCalls);
            public int MaximumConcurrentExports => Volatile.Read(ref _maximumConcurrentExports);

            public Task<ProcessResult> RunAsync(string fileName, string arguments, TimeSpan? timeout = null)
                => RunAsync(fileName, arguments, timeout, CancellationToken.None);

            public async Task<ProcessResult> RunAsync(
                string fileName,
                string arguments,
                TimeSpan? timeout,
                CancellationToken cancellationToken)
            {
                if (fileName.Equals("powershell.exe", StringComparison.OrdinalIgnoreCase))
                    return new ProcessResult { ExitCode = 0, Output = _infPath };
                if (!fileName.Equals("pnputil", StringComparison.OrdinalIgnoreCase))
                    return new ProcessResult { ExitCode = 1 };

                int active = Interlocked.Increment(ref _activeExports);
                Interlocked.Increment(ref _exportCalls);
                UpdateMaximum(active);
                try
                {
                    await Task.Delay(100, cancellationToken);
                    int lastQuote = arguments.LastIndexOf('"');
                    int previousQuote = arguments.LastIndexOf('"', lastQuote - 1);
                    string exportDirectory = arguments.Substring(previousQuote + 1, lastQuote - previousQuote - 1);
                    _fileSystem.AddFile(Path.Combine(exportDirectory, "serialized.inf"), new byte[] { 9, 8, 7 });
                    return new ProcessResult { ExitCode = 0, Output = "exported" };
                }
                finally
                {
                    Interlocked.Decrement(ref _activeExports);
                }
            }

            private void UpdateMaximum(int active)
            {
                int current;
                do
                {
                    current = Volatile.Read(ref _maximumConcurrentExports);
                    if (active <= current)
                        return;
                }
                while (Interlocked.CompareExchange(ref _maximumConcurrentExports, active, current) != current);
            }
        }

        [Fact]
        public async Task DriverPackageVerify_RejectsMalformedIdBeforeFileAccess()
        {
            Assert.False(await DriverPackageVerify.VerifyPackageAsync(
                Path.Combine(Path.GetTempPath(), "does-not-matter"), "short", 1));
            Assert.False(DriverPackageVerify.VerifyBytes(new byte[] { 1 }, "short", 1));
        }

        [Fact]
        public async Task DriverPackageService_CancellationStopsLocateProcess()
        {
            var service = new ShaPrint.Server.DriverPackageService(
                new CancellationProcessRunner(), new MockFileSystem());
            using var cancellation = new CancellationTokenSource(TimeSpan.FromMilliseconds(50));

            await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
                service.GetDriverPackageAsync("Test Driver", cancellation.Token));
        }

        private sealed class CancellationProcessRunner : IProcessRunner
        {
            public Task<ProcessResult> RunAsync(string fileName, string arguments, TimeSpan? timeout = null)
                => Task.FromResult(new ProcessResult { ExitCode = 1 });

            public async Task<ProcessResult> RunAsync(
                string fileName, string arguments, TimeSpan? timeout, CancellationToken cancellationToken)
            {
                await Task.Delay(Timeout.InfiniteTimeSpan, cancellationToken);
                return new ProcessResult { ExitCode = 1 };
            }
        }

        [Fact]
        public void ChunkSequence_RejectsDuplicateOutOfOrderAndGap()
        {
            var state = new DriverChunkSequence(3);
            Assert.True(state.TryAccept(0, 3, out _));
            Assert.False(state.TryAccept(0, 3, out _));
            Assert.False(state.TryAccept(2, 3, out _));
            Assert.True(state.TryAccept(1, 3, out _));
            Assert.True(state.TryAccept(2, 3, out _));
            Assert.True(state.IsComplete);
        }

        [Fact]
        public async Task RealProcessRunner_TimeoutReturnsFailureWithoutWaitingForOutput()
        {
            var runner = new RealProcessRunner();
            var started = DateTime.UtcNow;

            var result = await runner.RunAsync(
                OperatingSystem.IsWindows() ? "powershell.exe" : "sh",
                OperatingSystem.IsWindows() ? "-NoProfile -Command \"Start-Sleep -Seconds 30\"" : "-c \"sleep 30\"",
                TimeSpan.FromMilliseconds(200));

            Assert.False(result.Success);
            Assert.Contains("timed out", result.Error, StringComparison.OrdinalIgnoreCase);
            Assert.True(DateTime.UtcNow - started < TimeSpan.FromSeconds(5));
        }
    }
}
