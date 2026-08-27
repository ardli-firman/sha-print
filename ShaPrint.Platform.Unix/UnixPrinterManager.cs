using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Unix;

/// <summary>
/// <see cref="IPrinterManager"/> for macOS/Linux — CUPS, CLI-first (v1, no P/Invoke into
/// libcups). Enumeration via <c>lpstat -p</c>; printing via <c>lp -d &lt;name&gt; -t
/// &lt;title&gt; &lt;file&gt;</c> where the job bytes are staged to a temp file first
/// (never a shell redirect; see <see cref="UnixProcessRunner"/>).
///
/// Runtime guard: every public method throws <see cref="PlatformNotSupportedException"/>
/// when the process is not running on macOS/Linux.
/// </summary>
public sealed class UnixPrinterManager : IPrinterManager
{
    public async Task<List<PrinterInfo>> GetLocalPrintersAsync()
    {
        UnixProcessRunner.EnsureUnix();

        try
        {
            // lpstat -p prints one line per queue:
            //   "printer <name> is idle.  enabled since ..." / "... is disabled since ..."
            var result = await Task.Run(() => UnixProcessRunner.Run("lpstat", new[] { "-p" }));
            if (!result.Succeeded)
            {
                AppLogger.Error($"[PRINTER] lpstat -p failed (exit {result.ExitCode}): {result.StdErr.Trim()}");
                return new List<PrinterInfo>();
            }

            var printers = new List<PrinterInfo>();
            foreach (var line in result.StdOut.Split('\n', StringSplitOptions.RemoveEmptyEntries))
            {
                var parts = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (parts.Length < 3 || !parts[0].Equals("printer", StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                printers.Add(new PrinterInfo
                {
                    Name = parts[1],
                    // Keep the status text ("is idle. enabled since ...") as the description;
                    // per-printer driver resolution would need `lpstat -l -p <name>` per queue
                    // (N+1 processes) — deferred until a consumer needs it.
                    Description = string.Join(' ', parts.Skip(2)),
                    DriverName = string.Empty,
                });
            }

            AppLogger.Log($"[PRINTER] Enumerated {printers.Count} CUPS printer(s).");
            return printers;
        }
        catch (Exception ex)
        {
            AppLogger.Error("[PRINTER] Failed to enumerate printers: " + ex.Message);
            return new List<PrinterInfo>();
        }
    }

    public async Task<bool> PrintRawDataAsync(
        string printerName,
        byte[] data,
        string documentName,
        TimeSpan? timeout = null)
    {
        UnixProcessRunner.EnsureUnix();

        if (string.IsNullOrWhiteSpace(printerName))
        {
            AppLogger.Error("[PRINTER] PrintRawDataAsync requires a printer name.");
            return false;
        }

        if (data == null || data.Length == 0)
        {
            AppLogger.Error("[PRINTER] PrintRawDataAsync requires non-empty job data.");
            return false;
        }

        // Stage the job in a temp file (the CUPS tooling reads files, not raw pipes) and
        // always remove it afterwards.
        string tempFile = Path.Combine(Path.GetTempPath(), $"shaprint_job_{Guid.NewGuid():N}");
        try
        {
            await File.WriteAllBytesAsync(tempFile, data);
            var args = new List<string> { "-d", printerName, "-t", documentName, tempFile };
            var result = await Task.Run(() => UnixProcessRunner.Run("lp", args, timeout));

            if (!result.Succeeded)
            {
                AppLogger.Error($"[PRINTER] lp -d '{printerName}' failed (exit {result.ExitCode}): {result.StdErr.Trim()}");
                return false;
            }

            AppLogger.Log($"[PRINTER] lp accepted {data.Length} bytes for '{printerName}' (exit {result.ExitCode}).");
            return true;
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[PRINTER] Failed to print to '{printerName}': {ex.Message}");
            return false;
        }
        finally
        {
            try { File.Delete(tempFile); } catch { /* best-effort temp cleanup */ }
        }
    }
}
