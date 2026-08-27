using ShaPrint.Core;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Unix;

/// <summary>
/// <see cref="IVirtualPrinterManager"/> for macOS/Linux — CUPS, CLI-first (v1).
///
/// A "virtual printer" on Unix is a CUPS queue whose device URI points at the ShaPrint
/// backend (<c>shaprint://sha/&lt;url-encoded printer name&gt;</c>). The backend script
/// forwards every job to a ShaPrint server via the <c>shaprint send</c> CLI sender, which
/// relays the encrypted <c>PrintJobPayload</c> over TCP 9877 (Task 8 fills the CLI in; the
/// script already targets that contract).
///
/// Installed components (both root-owned system paths — see
/// <see cref="PrivilegeEscalationHelper"/>):
/// <list type="bullet">
///   <item><c>/usr/lib/cups/backend/shaprint</c> (executable backend script);</item>
///   <item><c>/usr/share/cups/model/shaprint/shaprint.ppd</c> (raw passthrough PPD, with a
///   user-writable temp fallback when the model dir is not writable).</item>
/// </list>
/// </summary>
public sealed class UnixVirtualPrinterManager : IVirtualPrinterManager
{
    /// <summary>Naming prefix for ShaPrint virtual queues (used by GetInstalledVirtualPrinters).</summary>
    private const string VirtualPrinterPrefix = "ShaPrint";

    /// <summary>
    /// Device URI scheme. CUPS backends receive the printer's device URI in the
    /// <c>DEVICE_URI</c> environment variable — NOT in the positional args — so the target
    /// printer name is URL-encoded into the URI path: <c>shaprint://sha/&lt;encoded&gt;</c>.
    /// "sha" is a fixed placeholder host (CUPS requires a host part in device URIs).
    /// </summary>
    private const string DeviceUriPrefix = "shaprint://sha/";

    /// <summary>CUPS backend directories across macOS and the main Linux distro families.</summary>
    private static readonly string[] BackendDirCandidates =
    {
        "/usr/lib/cups/backend",      // Debian/Ubuntu, openSUSE
        "/usr/libexec/cups/backend",  // macOS, RHEL/Fedora
        "/usr/lib64/cups/backend",    // RHEL/Fedora x64
    };

    private static string ResolveBackendDir()
        => BackendDirCandidates.FirstOrDefault(Directory.Exists) ?? BackendDirCandidates[0];

    private static string BackendPath => Path.Combine(ResolveBackendDir(), "shaprint");

    private const string ModelDir = "/usr/share/cups/model/shaprint";
    private const string PpdPath = ModelDir + "/shaprint.ppd";

    // ─────────────────────────────────────────────────────────────
    // CUPS backend script (stored as a resource string in the class —
    // no external file that must ship with the publish output).
    //
    // CUPS invokes backends as:  backend job-id user title copies options [filename]
    // The TARGET PRINTER is NOT among those arguments; it is decoded from DEVICE_URI
    // (shaprint://sha/<url-encoded name>). The job is staged to a temp file and forwarded
    // with `shaprint send --printer <name> --file <path>` — never discarded.
    // ─────────────────────────────────────────────────────────────
    private const string BackendScript = """
        #!/bin/bash
        # ShaPrint CUPS backend — forwards jobs to a ShaPrint server via the CLI sender.
        # CUPS invocation:  backend job-id user title copies options [filename]
        # The target printer is decoded from DEVICE_URI (shaprint://sha/<url-encoded name>),
        # NOT from the positional arguments (CUPS does not pass the printer name there).
        set -u

        # --- locate the CLI sender ------------------------------------------------
        cli="${SHAPRINT_CLI:-}"
        if [ -z "$cli" ]; then
          cli="$(command -v shaprint 2>/dev/null || true)"
        fi
        if [ -z "$cli" ] && [ -x "/usr/local/bin/shaprint" ]; then
          cli="/usr/local/bin/shaprint"
        fi
        if [ -z "$cli" ] && [ -n "${HOME:-}" ] && [ -x "$HOME/.local/bin/shaprint" ]; then
          cli="$HOME/.local/bin/shaprint"
        fi
        if [ -z "$cli" ]; then
          echo "ERROR: 'shaprint' CLI sender not found. Install ShaPrint or set SHAPRINT_CLI." >&2
          exit 1
        fi

        # --- decode the target printer from the device URI -----------------------
        # DEVICE_URI looks like:  shaprint://sha/<url-encoded printer name>
        uri="${DEVICE_URI:-}"
        if [ -z "$uri" ]; then
          echo "ERROR: DEVICE_URI is empty; cannot determine target printer." >&2
          exit 1
        fi
        case "$uri" in
          shaprint://*) ;;
          *) echo "ERROR: unexpected DEVICE_URI '$uri' (expected shaprint://...)." >&2; exit 1 ;;
        esac
        encoded="${uri#shaprint://}"
        # drop the fixed host placeholder: "sha/<encoded>" -> "<encoded>"
        encoded="${encoded#*/}"

        # bash-only percent-decoder (no python/sed dependency; works on macOS bash 3.2).
        decode_uri() {
          local enc="$1" out="" hex octal
          while [ -n "$enc" ]; do
            case "$enc" in
              %??*)
                hex="${enc:1:2}"
                octal="$(printf '%o' "$((16#$hex))" 2>/dev/null || true)"
                if [ -n "$octal" ]; then
                  out="${out}$(printf '%b' "\\$octal")"
                  enc="${enc:3}"
                else
                  out="${out}${enc:0:1}"; enc="${enc:1}"
                fi
                ;;
              *) out="${out}${enc:0:1}"; enc="${enc:1}" ;;
            esac
          done
          printf '%s' "$out"
        }

        printer="$(decode_uri "$encoded")"
        if [ -z "$printer" ]; then
          echo "ERROR: device URI does not contain a printer name." >&2
          exit 1
        fi

        # --- stage the job bytes ---------------------------------------------------
        tmp="$(mktemp "${TMPDIR:-/tmp}/shaprint_job_XXXXXX" 2>/dev/null)"
        if [ -z "$tmp" ] || [ ! -f "$tmp" ]; then
          echo "ERROR: could not create a temp file for the print job." >&2
          exit 1
        fi
        trap 'rm -f "$tmp"' EXIT

        if [ "$#" -ge 6 ] && [ -n "${6:-}" ] && [ -f "${6}" ]; then
          # CUPS passed a job file — copy it instead of reading stdin.
          cp "$6" "$tmp"
        else
          cat > "$tmp"
        fi

        # --- forward via the CLI sender --------------------------------------------
        # The job MUST reach the ShaPrint server (encrypted relay); this is not a
        # discard pattern. Exit code of the sender becomes the CUPS job result.
        "$cli" send --printer "$printer" --file "$tmp"
        rc=$?
        if [ "$rc" -ne 0 ]; then
          echo "ERROR: shaprint send failed for printer '$printer' (exit $rc)." >&2
        fi
        exit "$rc"
        """;

    // ─────────────────────────────────────────────────────────────
    // Raw passthrough PPD — mirrors CUPS' built-in `raw` model: the single
    // application/vnd.cups-raw filter makes CUPS skip conversion and hand the job bytes
    // to the backend unmodified (the ShaPrint server drives the actual rendering).
    // PageSize is declared (A4/Letter) for UI compatibility only.
    // ─────────────────────────────────────────────────────────────
    private const string Ppd = """
        *PPD-Adobe: "4.3"
        *FormatVersion: "4.3"
        *FileVersion: "1.0"
        *LanguageVersion: English
        *LanguageEncoding: ISOLatin1
        *PSVersion: "(3010.000) 0"
        *PCFileName: "shaprint.ppd"
        *Manufacturer: "ShaPrint"
        *Product: "(ShaPrint Virtual Printer)"
        *ModelName: "ShaPrint Virtual Printer"
        *ShortNickName: "ShaPrint Virtual Printer"
        *NickName: "ShaPrint Virtual Printer (raw passthrough)"
        *1284DeviceID: "MFG:ShaPrint;MDL:Virtual Printer;"
        *cupsFilter: "application/vnd.cups-raw 0 -"
        *OpenUI *PageSize: PickOne
        *OrderDependency: 10 AnySetup *PageSize
        *DefaultPageSize: A4
        *PageSize A4/A4: "<</PageSize[595 842]/ImagingBBox null>>setpagedevice"
        *PageSize Letter/Letter: "<</PageSize[612 792]/ImagingBBox null>>setpagedevice"
        *CloseUI: *PageSize
        *End
        """;

    /// <summary>Encodes a printer name into a CUPS device URI: shaprint://sha/&lt;url-encoded&gt;.</summary>
    internal static string BuildDeviceUri(string printerName)
        => DeviceUriPrefix + Uri.EscapeDataString(printerName);

    public Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string driverName)
    {
        UnixProcessRunner.EnsureUnix();

        if (string.IsNullOrWhiteSpace(virtualPrinterName))
        {
            return Task.FromResult((false, "Virtual printer name is required."));
        }
        if (string.IsNullOrWhiteSpace(driverName))
        {
            return Task.FromResult((false, "A driver must be selected."));
        }

        // 1) CUPS backend script -> root-owned /usr/lib/cups/backend (via elevation helper;
        //    no assumption that File.WriteAllText succeeds there).
        var backendWrite = PrivilegeEscalationHelper.WriteSystemFile(BackendPath, BackendScript, executable: true);
        if (!backendWrite.Success)
        {
            return Task.FromResult((false, $"Cannot install the CUPS backend at '{BackendPath}':\n{backendWrite.ErrorMessage}"));
        }

        // 2) PPD -> root-owned model dir. If that fails, fall back to a user-writable temp
        //    PPD (lpadmin only needs a readable PPD file) and keep going.
        string ppdArg;
        var ppdWrite = PrivilegeEscalationHelper.WriteSystemFile(PpdPath, Ppd, executable: false);
        if (ppdWrite.Success)
        {
            ppdArg = PpdPath;
        }
        else
        {
            ppdArg = WriteTempPpd();
            AppLogger.Log($"[VPRINTER] PPD install to '{PpdPath}' deferred to sudo (falling back to {ppdArg}).");
        }

        // 3) Register the CUPS queue. driverName is validated above but the physical PPD is
        //    always ShaPrint's raw passthrough PPD — the server drives the actual rendering
        //    (v1; driver-specific PPD selection is future work).
        string deviceUri = BuildDeviceUri(virtualPrinterName);
        var lpadmin = UnixProcessRunner.Run("lpadmin", new[] { "-p", virtualPrinterName, "-v", deviceUri, "-P", ppdArg, "-E" });
        if (!lpadmin.Succeeded)
        {
            string message = $"lpadmin failed (exit {lpadmin.ExitCode}): {lpadmin.StdErr.Trim()}";
            if (LooksLikePermissionFailure(lpadmin))
            {
                message += "\n" + PrivilegeEscalationHelper.ElevationInstruction(
                    $"lpadmin -p \"{virtualPrinterName}\" -v \"{deviceUri}\" -P \"{ppdArg}\" -E");
            }
            AppLogger.Error($"[VPRINTER] Install failed for '{virtualPrinterName}': {message}");
            return Task.FromResult((false, message));
        }

        AppLogger.Log($"[VPRINTER] Installed virtual printer '{virtualPrinterName}' (device {deviceUri}).");
        return Task.FromResult((true, string.Empty));
    }

    public Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string virtualPrinterName)
    {
        UnixProcessRunner.EnsureUnix();

        if (string.IsNullOrWhiteSpace(virtualPrinterName))
        {
            return Task.FromResult((false, "Virtual printer name is required."));
        }

        var lpadmin = UnixProcessRunner.Run("lpadmin", new[] { "-x", virtualPrinterName });
        if (!lpadmin.Succeeded)
        {
            string message = $"lpadmin -x '{virtualPrinterName}' failed (exit {lpadmin.ExitCode}): {lpadmin.StdErr.Trim()}";
            if (LooksLikePermissionFailure(lpadmin))
            {
                message += "\n" + PrivilegeEscalationHelper.ElevationInstruction($"lpadmin -x \"{virtualPrinterName}\"");
            }
            AppLogger.Error($"[VPRINTER] Remove failed for '{virtualPrinterName}': {message}");
            return Task.FromResult((false, message));
        }

        AppLogger.Log($"[VPRINTER] Removed virtual printer '{virtualPrinterName}'.");
        return Task.FromResult((true, string.Empty));
    }

    public bool CheckPrinterExists(string printerName)
    {
        UnixProcessRunner.EnsureUnix();

        if (string.IsNullOrWhiteSpace(printerName))
        {
            return false;
        }

        // `lpstat -p <name>` exits 0 when the destination is known, 1 otherwise
        // ("lpstat: Unknown destination").
        var result = UnixProcessRunner.Run("lpstat", new[] { "-p", printerName });
        return result.Succeeded;
    }

    public List<string> GetInstalledDrivers()
    {
        UnixProcessRunner.EnsureUnix();

        try
        {
            // lpinfo -m prints "<ppd-id-or-drv-uri> <model description>" per line.
            var result = UnixProcessRunner.Run("lpinfo", new[] { "-m" });
            if (!result.Succeeded)
            {
                AppLogger.Error($"[VPRINTER] lpinfo -m failed (exit {result.ExitCode}): {result.StdErr.Trim()}");
                return new List<string>();
            }

            var drivers = new List<string>();
            foreach (var line in result.StdOut.Split('\n', StringSplitOptions.RemoveEmptyEntries))
            {
                int space = line.IndexOf(' ');
                if (space <= 0)
                {
                    continue;
                }
                drivers.Add(line[(space + 1)..].Trim()); // model description (user-facing)
            }

            AppLogger.Log($"[VPRINTER] Enumerated {drivers.Count} CUPS driver(s) via lpinfo -m.");
            return drivers;
        }
        catch (Exception ex)
        {
            AppLogger.Error("[VPRINTER] Failed to enumerate drivers: " + ex.Message);
            return new List<string>();
        }
    }

    public List<string> GetInstalledVirtualPrinters()
    {
        UnixProcessRunner.EnsureUnix();

        try
        {
            var result = UnixProcessRunner.Run("lpstat", new[] { "-p" });
            if (!result.Succeeded)
            {
                AppLogger.Error($"[VPRINTER] lpstat -p failed (exit {result.ExitCode}): {result.StdErr.Trim()}");
                return new List<string>();
            }

            var printers = new List<string>();
            foreach (var line in result.StdOut.Split('\n', StringSplitOptions.RemoveEmptyEntries))
            {
                var parts = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (parts.Length < 2 || !parts[0].Equals("printer", StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                string name = parts[1];
                if (name.StartsWith(VirtualPrinterPrefix, StringComparison.OrdinalIgnoreCase))
                {
                    printers.Add(name);
                }
            }

            printers.Sort(StringComparer.OrdinalIgnoreCase);
            return printers;
        }
        catch (Exception ex)
        {
            AppLogger.Error("[VPRINTER] Failed to enumerate virtual printers: " + ex.Message);
            return new List<string>();
        }
    }

    private static string WriteTempPpd()
    {
        string path = Path.Combine(Path.GetTempPath(), $"shaprint_{Guid.NewGuid():N}.ppd");
        File.WriteAllText(path, Ppd);
        return path;
    }

    private static bool LooksLikePermissionFailure(ProcessResult result)
    {
        if (PrivilegeEscalationHelper.IsRunningAsRoot())
        {
            return false;
        }

        string err = result.StdErr;
        return err.Contains("permission", StringComparison.OrdinalIgnoreCase) ||
               err.Contains("forbidden", StringComparison.OrdinalIgnoreCase) ||
               err.Contains("denied", StringComparison.OrdinalIgnoreCase) ||
               err.Contains("not authorized", StringComparison.OrdinalIgnoreCase);
    }
}
