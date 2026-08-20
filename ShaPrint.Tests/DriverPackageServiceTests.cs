using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using ShaPrint.Core.Abstractions;
using ShaPrint.Core.Network;
using ShaPrint.Server;
using Xunit;

namespace ShaPrint.Tests
{
    /// <summary>
    /// Unit tests for DriverPackageService covering the happy-path package export,
    /// exact INF path resolution, and cache behavior.
    /// </summary>
    public class DriverPackageServiceTests
    {
        private class TestProcessRunner : IProcessRunner
        {
            public List<(string FileName, string Arguments)> ExecutedCommands { get; } = new();
            public Func<string, string, ProcessResult>? CommandHandler { get; set; }

            public Task<ProcessResult> RunAsync(string fileName, string arguments, TimeSpan? timeout = null)
            {
                ExecutedCommands.Add((fileName, arguments));
                if (CommandHandler != null)
                {
                    return Task.FromResult(CommandHandler(fileName, arguments));
                }

                return Task.FromResult(new ProcessResult { ExitCode = 0, Output = string.Empty });
            }
        }

        private class TestFileSystem : IFileSystem
        {
            private readonly Dictionary<string, byte[]> _files = new(StringComparer.OrdinalIgnoreCase);
            private readonly HashSet<string> _directories = new(StringComparer.OrdinalIgnoreCase);

            public void AddFile(string path, byte[] content)
            {
                _files[path] = content;
                var dir = Path.GetDirectoryName(path);
                if (!string.IsNullOrEmpty(dir))
                    _directories.Add(dir);
            }

            public Task WriteAllBytesAsync(string path, byte[] data)
            {
                _files[path] = data;
                var dir = Path.GetDirectoryName(path);
                if (!string.IsNullOrEmpty(dir))
                    _directories.Add(dir);
                return Task.CompletedTask;
            }

            public Task<byte[]> ReadAllBytesAsync(string path)
            {
                if (_files.TryGetValue(path, out var data))
                    return Task.FromResult(data);
                throw new FileNotFoundException(path);
            }

            public bool FileExists(string path) => _files.ContainsKey(path);
            public bool DirectoryExists(string path) => _directories.Contains(path);
            public void CreateDirectory(string path) => _directories.Add(path);
            public long GetFileSize(string path) => _files.TryGetValue(path, out var d) ? d.Length : 0;
            public void DeleteFile(string path) => _files.Remove(path);

            public void DeleteDirectory(string path, bool recursive)
            {
                _directories.Remove(path);
                if (recursive)
                {
                    var prefix = path.EndsWith(Path.DirectorySeparatorChar.ToString()) ? path : path + Path.DirectorySeparatorChar;
                    var altPrefix = path.EndsWith("/") || path.EndsWith("\\") ? path : path + "/";
                    var toRemove = _files.Keys.Where(k =>
                        k.StartsWith(prefix, StringComparison.OrdinalIgnoreCase) ||
                        k.StartsWith(altPrefix, StringComparison.OrdinalIgnoreCase)).ToList();
                    foreach (var k in toRemove) _files.Remove(k);
                }
            }

            public string[] GetFiles(string path, string searchPattern, SearchOption searchOption)
            {
                var normalizedPath = path.TrimEnd('/', '\\');
                var query = _files.Keys.Where(k =>
                {
                    var fileDir = Path.GetDirectoryName(k)?.TrimEnd('/', '\\');
                    if (searchOption == SearchOption.TopDirectoryOnly)
                    {
                        return string.Equals(fileDir, normalizedPath, StringComparison.OrdinalIgnoreCase);
                    }
                    return k.StartsWith(normalizedPath, StringComparison.OrdinalIgnoreCase);
                });
                return query.ToArray();
            }
        }

        [Fact]
        public async Task GetDriverPackageAsync_HappyCase_ResolvesInfExportsPackageAndReturnsExpectedSha256()
        {
            // Arrange
            var mockFs = new TestFileSystem();
            var mockProcess = new TestProcessRunner();

            string driverName = "EPSON L120 Series";
            string infPath = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows), "INF", "oem25.inf");
            byte[] infContent = Encoding.UTF8.GetBytes("; Mock INF file content for EPSON L120 Series\nDriverVer=01/01/2024,1.0.0.0");
            mockFs.AddFile(infPath, infContent);

            string cacheRoot = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "ShaPrint", "DriverCache");
            string infHash = Convert.ToHexString(SHA256.HashData(infContent)).ToLowerInvariant();
            string exportDir = Path.Combine(cacheRoot, infHash);

            byte[] driverSysContent = new byte[] { 0x4D, 0x5A, 0x90, 0x00, 0x03, 0x00 };
            byte[] driverCatContent = new byte[] { 0x30, 0x82, 0x02, 0x00 };

            mockProcess.CommandHandler = (fileName, args) =>
            {
                if (fileName.Equals("powershell.exe", StringComparison.OrdinalIgnoreCase))
                {
                    // Verify exact equality query in PowerShell command
                    Assert.Contains($"$_.Name -eq '{driverName}'", args);
                    return new ProcessResult
                    {
                        ExitCode = 0,
                        Output = infPath + Environment.NewLine
                    };
                }

                if (fileName.Equals("pnputil", StringComparison.OrdinalIgnoreCase))
                {
                    Assert.Contains("/export-driver", args);
                    // Simulate pnputil exporting the driver files into exportDir
                    mockFs.AddFile(Path.Combine(exportDir, "oem25.inf"), infContent);
                    mockFs.AddFile(Path.Combine(exportDir, "epson.sys"), driverSysContent);
                    mockFs.AddFile(Path.Combine(exportDir, "epson.cat"), driverCatContent);

                    return new ProcessResult
                    {
                        ExitCode = 0,
                        Output = "Driver package exported successfully."
                    };
                }

                return new ProcessResult { ExitCode = 1, Output = "Unexpected command" };
            };

            var service = new DriverPackageService(mockProcess, mockFs);

            // Act
            var manifest = await service.GetDriverPackageAsync(driverName);

            // Assert
            Assert.NotNull(manifest);
            Assert.Equal("oem25.inf", manifest.InfName);
            Assert.Equal(driverName, manifest.DriverName);
            Assert.Equal(3, manifest.FileCount);
            Assert.True(manifest.TotalSizeBytes > 0);
            Assert.NotEmpty(manifest.Sha256);

            // Verify the generated zip archive and SHA-256 match the package files
            string zipPath = Path.Combine(exportDir, "package.zip");
            Assert.True(mockFs.FileExists(zipPath));
            byte[] actualZipBytes = await mockFs.ReadAllBytesAsync(zipPath);
            string actualSha256 = Convert.ToHexString(SHA256.HashData(actualZipBytes)).ToLowerInvariant();
            Assert.Equal(actualSha256, manifest.Sha256);
            Assert.Equal(actualZipBytes.Length, manifest.TotalSizeBytes);

            // Verify manifest.json was written
            string manifestPath = Path.Combine(exportDir, "manifest.json");
            Assert.True(mockFs.FileExists(manifestPath));

            // Verify subsequent call uses in-memory cache and doesn't re-invoke commands
            int commandCount = mockProcess.ExecutedCommands.Count;
            var cachedManifest = await service.GetDriverPackageAsync(driverName);
            Assert.NotNull(cachedManifest);
            Assert.Same(manifest, cachedManifest);
            Assert.Equal(commandCount, mockProcess.ExecutedCommands.Count);
        }

        [Fact]
        public async Task LocateInfPathAsync_UsesExactDriverMatching_DoesNotMatchPartial()
        {
            // Arrange
            var mockFs = new TestFileSystem();
            var mockProcess = new TestProcessRunner();

            string driverName = "EPSON L120";

            mockProcess.CommandHandler = (fileName, args) =>
            {
                if (fileName.Equals("powershell.exe", StringComparison.OrdinalIgnoreCase))
                {
                    // Must be exact match $_.Name -eq 'EPSON L120', not -like '*EPSON L120*'
                    Assert.Contains($"$_.Name -eq '{driverName}'", args);
                    Assert.DoesNotContain($"*{driverName}*", args);
                    return new ProcessResult
                    {
                        ExitCode = 0,
                        Output = string.Empty
                    };
                }

                return new ProcessResult { ExitCode = 1, Output = "Not found" };
            };

            var service = new DriverPackageService(mockProcess, mockFs);

            // Act
            var manifest = await service.GetDriverPackageAsync(driverName);

            // Assert: WMI failed and fallback failed -> returns null
            Assert.Null(manifest);
            Assert.Contains(mockProcess.ExecutedCommands, c => c.FileName == "powershell.exe" && c.Arguments.Contains($"$_.Name -eq '{driverName}'"));
        }
    }
}
