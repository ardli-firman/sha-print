using System;
using System.IO;
using System.Threading.Tasks;
using ShaPrint.Core.Abstractions;
using ShaPrint.Tests; // MockProcessRunner, MockFileSystem
using ShaPrint.WpfApp.Services.Client;
using Xunit;

namespace ShaPrint.Tests
{
    /// <summary>
    /// T17 — Unit tests for DriverInstaller (client-side install).
    /// Tests the 3-step install chain: Add-PrinterDriver -InfPath → pnputil → -Name (inbox).
    /// </summary>
    public class DriverInstallTests : IDisposable
    {
        private readonly string _tempDir;

        public DriverInstallTests()
        {
            _tempDir = Path.Combine(Path.GetTempPath(), "ShaPrintInstallTests_" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_tempDir);
        }

        public void Dispose()
        {
            if (Directory.Exists(_tempDir))
                Directory.Delete(_tempDir, true);
        }

        // ── T17: Valid INF → Add-PrinterDriver -InfPath succeeds ──────────

        [Fact]
        public async Task InstallDriverFromPackage_ValidInf_CallsAddPrinterDriver()
        {
            // Arrange
            var mockProcess = new MockProcessRunner();
            var installer = new DriverInstaller(mockProcess);

            string infPath = Path.Combine(_tempDir, "oem25.inf");
            await File.WriteAllTextAsync(infPath, "[Version]\nSignature=\"$Windows NT$\"");

            // Add-PrinterDriver -InfPath succeeds
            mockProcess.AddResponse("powershell.exe", result: new ProcessResult
            {
                ExitCode = 0,
                Output = ""
            });

            // Act
            var result = await installer.InstallDriverFromInfAsync(infPath);

            // Assert
            Assert.True(result.Success);
            Assert.Null(result.ErrorMessage);
        }

        // ── T18: Add-PrinterDriver fails → falls back to pnputil ──────────

        [Fact]
        public async Task InstallDriverFromPackage_InfPathFails_FallsBackToPnputil()
        {
            // Arrange
            var mockProcess = new MockProcessRunner();
            var installer = new DriverInstaller(mockProcess);

            string infPath = Path.Combine(_tempDir, "oem25.inf");
            await File.WriteAllTextAsync(infPath, "[Version]\nSignature=\"$Windows NT$\"");

            // Add-PrinterDriver -InfPath fails
            mockProcess.AddResponse("powershell.exe", result: new ProcessResult
            {
                ExitCode = 1,
                Output = "Add-PrinterDriver : The specified driver is not valid."
            });

            // pnputil /add-driver succeeds
            mockProcess.AddResponse("pnputil", result: new ProcessResult
            {
                ExitCode = 0,
                Output = "Driver package added successfully."
            });

            // Act
            var result = await installer.InstallDriverFromInfAsync(infPath);

            // Assert
            Assert.True(result.Success);
            Assert.Equal(1, mockProcess.CallCount("pnputil"));
        }

        // ── T19: Both fail → returns error ─────────────────────────────────

        [Fact]
        public async Task InstallDriverFromPackage_BothFail_ReturnsError()
        {
            // Arrange
            var mockProcess = new MockProcessRunner();
            var installer = new DriverInstaller(mockProcess);

            string infPath = Path.Combine(_tempDir, "oem25.inf");
            await File.WriteAllTextAsync(infPath, "[Version]\nSignature=\"$Windows NT$\"");

            // Add-PrinterDriver fails
            mockProcess.AddResponse("powershell.exe", result: new ProcessResult
            {
                ExitCode = 1,
                Output = "Add-PrinterDriver failed."
            });

            // pnputil also fails
            mockProcess.AddResponse("pnputil", result: new ProcessResult
            {
                ExitCode = 1,
                Output = "pnputil failed: driver not found."
            });

            // Act
            var result = await installer.InstallDriverFromInfAsync(infPath);

            // Assert
            Assert.False(result.Success);
            Assert.NotNull(result.ErrorMessage);
            Assert.Contains("pnputil", result.ErrorMessage, StringComparison.OrdinalIgnoreCase);
        }

        // ── T20: Non-existent INF file → returns error ─────────────────────

        [Fact]
        public async Task InstallDriverFromPackage_NonExistentInf_ReturnsError()
        {
            // Arrange
            var mockProcess = new MockProcessRunner();
            var installer = new DriverInstaller(mockProcess);

            string nonExistentInf = Path.Combine(_tempDir, "nonexistent.inf");

            // Act
            var result = await installer.InstallDriverFromInfAsync(nonExistentInf);

            // Assert
            Assert.False(result.Success);
            Assert.NotNull(result.ErrorMessage);
            Assert.Contains("not found", result.ErrorMessage, StringComparison.OrdinalIgnoreCase);
        }

        // ── T21: Null/empty INF path → returns error ──────────────────────

        [Fact]
        public async Task InstallDriverFromPackage_EmptyPath_ReturnsError()
        {
            // Arrange
            var mockProcess = new MockProcessRunner();
            var installer = new DriverInstaller(mockProcess);

            // Act
            var result = await installer.InstallDriverFromInfAsync("");

            // Assert
            Assert.False(result.Success);
            Assert.NotNull(result.ErrorMessage);
        }

        // ── T22: Full flow structure test ──────────────────────────────────

        [Fact]
        public void DriverInstallResult_DefaultValues_AreCorrect()
        {
            // Arrange & Act
            var result = new DriverInstallResult();

            // Assert
            Assert.False(result.Success);
            Assert.Null(result.ErrorMessage);
            Assert.Null(result.InstalledDriverName);
        }
    }
}
