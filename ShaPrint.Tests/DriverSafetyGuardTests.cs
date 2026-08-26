using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading.Tasks;
using ShaPrint.Core.Abstractions;
using ShaPrint.Platform.Windows;
using Xunit;

namespace ShaPrint.Tests
{
    public class DriverSafetyGuardTests : IDisposable
    {
        private readonly string _tempDir;

        public DriverSafetyGuardTests()
        {
            _tempDir = Path.Combine(Path.GetTempPath(), "ShaPrintSafetyTests_" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_tempDir);
        }

        public void Dispose()
        {
            if (Directory.Exists(_tempDir))
            {
                try { Directory.Delete(_tempDir, true); } catch { }
            }
        }

        [Fact]
        public void ValidateArchitecture_SameArchitecture_ReturnsTrue()
        {
            string currentArch = RuntimeInformation.OSArchitecture.ToString();
            bool valid = DriverSafetyGuard.ValidateArchitecture(currentArch, out string? error);

            Assert.True(valid);
            Assert.Null(error);
        }

        [Fact]
        public void ValidateArchitecture_NullOrEmpty_ReturnsTrueForLegacy()
        {
            bool validNull = DriverSafetyGuard.ValidateArchitecture(null, out string? errorNull);
            bool validEmpty = DriverSafetyGuard.ValidateArchitecture("", out string? errorEmpty);

            Assert.True(validNull);
            Assert.True(validEmpty);
        }

        [Fact]
        public void ValidateArchitecture_MismatchedArchitecture_ReturnsFalseWithError()
        {
            string mismatched = RuntimeInformation.OSArchitecture == Architecture.X64 ? "X86" : "X64";
            bool valid = DriverSafetyGuard.ValidateArchitecture(mismatched, out string? error);

            Assert.False(valid);
            Assert.NotNull(error);
            Assert.Contains("Driver architecture mismatch", error);
        }

        [Fact]
        public void ValidateInfSafety_ValidPrinterInf_ReturnsTrue()
        {
            string infPath = Path.Combine(_tempDir, "valid_printer.inf");
            File.WriteAllText(infPath, @"
[Version]
Signature=""$Windows NT$""
Class=Printer
ClassGUID={4D36E979-E325-11CE-BFC1-08002BE10318}
Provider=%Epson%
DriverVer=01/01/2022,1.0.0.0

[Manufacturer]
%Epson%=Epson,NTamd64

[Epson.NTamd64]
""EPSON L3210 Series"" = L3210_Install
");

            bool valid = DriverSafetyGuard.ValidateInfSafety(infPath, out string? error);

            Assert.True(valid);
            Assert.Null(error);
        }

        [Fact]
        public void ValidateInfSafety_MissingVersionSection_ReturnsFalse()
        {
            string infPath = Path.Combine(_tempDir, "no_version.inf");
            File.WriteAllText(infPath, @"
Class=Printer
[Manufacturer]
%Epson%=Epson
");

            bool valid = DriverSafetyGuard.ValidateInfSafety(infPath, out string? error);

            Assert.False(valid);
            Assert.Contains("missing required [Version]", error);
        }

        [Fact]
        public void ValidateInfSafety_DangerousSystemClass_ReturnsFalse()
        {
            string infPath = Path.Combine(_tempDir, "dangerous_system.inf");
            File.WriteAllText(infPath, @"
[Version]
Signature=""$Windows NT$""
Class=System
ClassGUID={4D36E97D-E325-11CE-BFC1-08002BE10318}
");

            bool valid = DriverSafetyGuard.ValidateInfSafety(infPath, out string? error);

            Assert.False(valid);
            Assert.Contains("non-printer system device class", error);
        }

        [Fact]
        public void ValidateInfSafety_KernelServiceDeclaration_ReturnsFalse()
        {
            string infPath = Path.Combine(_tempDir, "kernel_service.inf");
            File.WriteAllText(infPath, @"
[Version]
Signature=""$Windows NT$""
Class=Printer

[Install_Service]
ServiceType=1
StartType=0
");

            bool valid = DriverSafetyGuard.ValidateInfSafety(infPath, out string? error);

            Assert.False(valid);
            Assert.Contains("kernel-mode / boot-start", error);
        }

        [Fact]
        public void ValidateInfSafety_EmptyOrMissingFile_ReturnsFalse()
        {
            string emptyPath = Path.Combine(_tempDir, "empty.inf");
            File.WriteAllText(emptyPath, "");

            bool emptyValid = DriverSafetyGuard.ValidateInfSafety(emptyPath, out string? emptyError);
            bool missingValid = DriverSafetyGuard.ValidateInfSafety(Path.Combine(_tempDir, "nonexistent.inf"), out string? missingError);

            Assert.False(emptyValid);
            Assert.False(missingValid);
        }

        [Fact]
        public async Task EnforceDriverIsolation_InvokesPowerShellSetPrinterDriver()
        {
            var mockRunner = new MockProcessRunner();
            mockRunner.AddResponse("powershell.exe", result: new ProcessResult
            {
                ExitCode = 0,
                Output = "OK"
            });

            await DriverSafetyGuard.EnforceDriverIsolationAsync(mockRunner, "EPSON L3210 Series");

            Assert.True(mockRunner.CallCount("powershell.exe") > 0);
        }

        [Fact]
        public async Task EnsureSpoolerHealthy_WhenStopped_AttemptsStartService()
        {
            var mockRunner = new MockProcessRunner();
            // First call: Get-Service returns Stopped
            mockRunner.AddResponse("powershell.exe", result: new ProcessResult
            {
                ExitCode = 0,
                Output = "Stopped"
            });
            // Second call: Start-Service
            mockRunner.AddResponse("powershell.exe", result: new ProcessResult
            {
                ExitCode = 0,
                Output = ""
            });

            await DriverSafetyGuard.EnsureSpoolerHealthyAsync(mockRunner);

            Assert.True(mockRunner.CallCount("powershell.exe") >= 2);
        }
    }
}
