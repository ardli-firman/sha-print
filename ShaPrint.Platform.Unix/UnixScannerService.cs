using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Unix;

/// <summary>
/// <see cref="IScannerService"/> for macOS/Linux — SANE via the <c>scanimage</c> CLI
/// (v1; no P/Invoke into libsane). macOS requires <c>brew install sane-backends</c>,
/// Linux the <c>sane-utils</c> package.
///
/// Scan output is read through <see cref="UnixProcessRunner.RunBinaryOutput"/> (redirected
/// stdout stream, copied by C#) — the <c>&gt;</c> shell redirect is deliberately NOT used.
/// </summary>
public sealed class UnixScannerService : IScannerService
{
    public List<ScannerInfo> GetLocalScanners()
    {
        UnixProcessRunner.EnsureUnix();

        try
        {
            // scanimage -L output lines:
            //   device `genesys:libusb:001:003' is a Canon CanoScan LiDE 220 flatbed scanner
            //   device `airscan:e0:Canon TS9100 series' is a Canon TS9100 series network scanner
            var result = UnixProcessRunner.Run("scanimage", new[] { "-L" });
            if (!result.Succeeded)
            {
                AppLogger.Error($"[SCAN] scanimage -L failed (exit {result.ExitCode}): {result.StdErr.Trim()}");
                return new List<ScannerInfo>();
            }

            var scanners = new List<ScannerInfo>();
            foreach (var line in result.StdOut.Split('\n', StringSplitOptions.RemoveEmptyEntries))
            {
                int nameStart = line.IndexOf('`');
                int nameEnd = line.IndexOf('\'', nameStart + 1);
                if (nameStart < 0 || nameEnd < 0)
                {
                    continue;
                }

                string name = line[(nameStart + 1)..nameEnd];
                if (string.IsNullOrWhiteSpace(name))
                {
                    continue;
                }

                int isIdx = line.IndexOf(" is ", nameEnd, StringComparison.Ordinal);
                string description = isIdx >= 0 ? line[(isIdx + 4)..].Trim() : string.Empty;

                scanners.Add(new ScannerInfo { Name = name, Description = description });
            }

            AppLogger.Log($"[SCAN] Enumerated {scanners.Count} scanner(s) via scanimage -L.");
            return scanners;
        }
        catch (Exception ex)
        {
            AppLogger.Error("[SCAN] Failed to enumerate scanners: " + ex.Message);
            return new List<ScannerInfo>();
        }
    }

    public byte[] PerformScan(
        string scannerName,
        int dpi,
        int colorMode,
        string format,
        out string actualFormat)
    {
        UnixProcessRunner.EnsureUnix();
        actualFormat = string.Empty;

        if (string.IsNullOrWhiteSpace(scannerName))
        {
            AppLogger.Error("[SCAN] PerformScan requires a scanner name.");
            return Array.Empty<byte>();
        }

        string fmt = NormalizeFormat(format);
        string mode = colorMode switch
        {
            0 => "Lineart",
            1 => "Gray",
            _ => "Color",
        };

        var args = new List<string>
        {
            "-d", scannerName,
            $"--format={fmt}",
            $"--mode={mode}",
            $"--resolution={dpi}",
        };

        try
        {
            var (bytes, stderr, exitCode) =
                UnixProcessRunner.RunBinaryOutput("scanimage", args, TimeSpan.FromMinutes(5));

            if (exitCode != 0 || bytes.Length == 0)
            {
                AppLogger.Error($"[SCAN] scanimage failed (exit {exitCode}): {stderr.Trim()}");
                return Array.Empty<byte>();
            }

            actualFormat = fmt;
            AppLogger.Log($"[SCAN] Captured {bytes.Length} bytes from '{scannerName}' ({dpi}dpi, {mode}, .{fmt}).");
            return bytes;
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[SCAN] PerformScan failed: {ex.Message}");
            return Array.Empty<byte>();
        }
    }

    private static string NormalizeFormat(string format)
        => format.Trim().ToLowerInvariant() switch
        {
            "jpeg" or "jpg" => "jpeg",
            "pdf" => "pdf",
            "tiff" => "tiff",
            "pnm" => "pnm",
            _ => "png",
        };
}
