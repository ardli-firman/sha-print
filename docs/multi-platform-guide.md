# ShaPrint Multi-Platform Guide

> ShaPrint v2 moves beyond Windows-only: the same .NET 8 codebase now ships a **Windows
> desktop app (WPF)**, an **Avalonia UI** that runs on Windows/macOS/Linux, a
> **CLI-first CUPS/SANE backend** for macOS and Linux, and a **print-only Android client**.
> `ShaPrint.Core` (networking protocol, crypto, security model) is unchanged and 100%
> shared across every platform.

**Status:** Windows = production-ready (the shipped WpfApp). macOS/Linux = v1 (CLI-first
CUPS/SANE, **not yet verified on real hardware** — see
[Not verified / Future Work](#not-verified--future-work)). Android = print-only client,
Release APK builds green in CI.

- Design spec: `docs/superpowers/specs/2026-08-24-multi-platform-design.md`
- Implementation plan: `docs/superpowers/plans/2026-08-24-multi-platform.md`

---

## Platform Feature Matrix

| Feature | Windows (WpfApp, shipped) | Windows (Avalonia UI) | macOS (v1) | Linux (v1) | Android |
|---|---|---|---|---|---|
| **Server** (host physical printer) | Yes | Yes | Yes (CUPS) | Yes (CUPS) | — |
| **Client** (virtual printer) | Yes | Yes | Yes (CUPS backend) | Yes (CUPS backend) | — |
| **Print-only client** (send file) | — | — | — | — | **Yes** |
| **Monitor** (dashboard) | Yes | Yes | Yes | Yes | — |
| **Scan** | Yes (WIA 2.0) | Yes (WIA 2.0) | Yes (SANE `scanimage`) | Yes (SANE `scanimage`) | — |
| **Auto-update** (GitHub) | Yes | Yes | Yes (implemented; unverified) | Yes (implemented; unverified) | — |
| **Virtual printer install** | Yes | Yes | Yes (needs root) | Yes (needs root) | — |
| **Driver provisioning** | Yes (multi-vendor + `DriverSafetyGuard`) | Yes | `lpinfo -m` list; raw PPD in v1 | `lpinfo -m` list; raw PPD in v1 | — |
| **CLI sender `shaprint send`** | Yes | Yes | Yes | Yes | — |

Notes:

- **Android** is deliberately *print-only*: pick a server (UDP discovery), pick a file
  (Storage Access Framework), and relay it to the server over TCP 9877. There is no
  virtual printer, scanner, or monitor on Android.
- **macOS/Linux v1 driver provisioning** installs the ShaPrint raw passthrough PPD for
  every queue; the server drives the actual rendering. Driver-specific PPD selection is
  future work.
- **Update** on macOS/Linux reuses the shared GitHub-based `UpdateService` (it launches a
  per-OS `ShaPrint.Updater` binary), but the full self-update cycle has **not been
  verified on real macOS/Linux machines**.

---

## Ports & Security Model (unchanged)

| Port | Protocol | Purpose |
|------|----------|---------|
| 9876 | UDP | Service discovery (broadcast, HMAC-SHA256 signed) |
| 9877 | TCP | Print / scan data transfer (AES-256-GCM `PrintJobPayload`) |
| 9878 | TCP | Monitor status query |

All clients must share the same **Network Channel** secret as the server — every key is
derived from it (PBKDF2, 100k iterations). Discovery responses are HMAC-verified before
being trusted; print payloads are AES-256-GCM encrypted end to end.

---

## How a Print Job Flows (per platform)

### Windows (Client -> Server)

```
Client PC                                          Server PC
+------------------+  Ctrl+P  +---------------------------+  +---------------------------+
| App (Word,       | -------> | ShaPrint virtual printer   |  | ShaPrint Server mode      |
| Chrome, Acrobat) |          | (Spooler virtual port)     |  | listening on TCP 9877     |
+------------------+          |                           |  | receives PrintJobPayload  |
                              | PipeListener captures the |  | -> SpoolerApi -> physical |
                              | raw spool bytes           |  |    printer                |
                              | -> IPrintRelayClient      |  +---------------------------+
                              | -> TCP 9877 (AES-256-GCM) |
                              +---------------------------+
```

The Windows virtual printer install path keeps the full multi-vendor provisioning
pipeline (Canon/HP/Brother driver packages, INF selection, signature/package verification,
safe ZIP extraction) plus **`DriverSafetyGuard`** — the guard that prevents
BSOD/kernel-corruption/spooler-deadlock scenarios. Do not disable it.

### macOS / Linux (CUPS backend -> CLI sender -> Server)

```
Client (macOS/Linux)                                   Server (any platform)
+----------------------------+  Ctrl+P  +-----------------------------+  +-----------------------------+
| App (any)                  | -------> | ShaPrint CUPS queue         |  | ShaPrint Server mode        |
|                            |          | (raw passthrough PPD)      |  | listening on TCP 9877       |
+----------------------------+          |                             |  | receives PrintJobPayload    |
                                        | /usr/lib/cups/backend/     |  | -> physical printer         |
                                        |    shaprint                 |  +-----------------------------+
                                        | DEVICE_URI =                |
                                        |    shaprint://sha/<name>    |
                                        | stage job to temp file      |
                                        | -> `shaprint send --printer |
                                        |    <name> --file <tmp>`     |
                                        | -> UDP 9876 discovery (or   |
                                        |    --host) -> TCP 9877      |
                                        | -> temp file deleted; exit  |
                                        |    code = CUPS job result   |
                                        +-----------------------------+
```

### Android (file picker -> relay -> Server)

```
Android device                                    Server (any platform)
+------------------+  1. SAF picker (ACTION_OPEN_DOCUMENT) -> PDF / image
| ShaPrint app     |  2. UDP 9876 discovery (WifiManager MulticastLock
| (print-only)     |     held for the duration of the scan)
+------------------+  3. user picks target printer
                    4. TCP 9877 (AES-256-GCM PrintJobPayload) ---->  +--------------------------+
                                                                     | ShaPrint Server mode     |
                                                                     | listening on TCP 9877    |
                                                                     | receives PrintJobPayload |
                                                                     | -> physical printer      |
                                                                     +--------------------------+
```

---

## Windows

### Build & run the shipped app (ShaPrint.WpfApp)

The WPF app is the Windows production build (net8.0-windows10.0.17763, `DriverSafetyGuard`
+ multi-vendor provisioning intact):

```bash
# run from source (Windows, .NET 8 SDK required)
dotnet run --project ShaPrint.WpfApp/ShaPrint.WpfApp.csproj

# publish a self-contained single-file build (no .NET runtime needed on target)
dotnet publish ShaPrint.WpfApp/ShaPrint.WpfApp.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true
```

### Optional: the Avalonia UI on Windows

The shared Avalonia shell (`ShaPrint.UI`) also runs on Windows. Use the **full** Windows
TFM — the project targets `net8.0-windows10.0.17763`, so the shorthand `-f net8.0-windows`
does **not** resolve:

```bash
dotnet run --project ShaPrint.UI/ShaPrint.UI.csproj -f net8.0-windows10.0.17763
```

The Windows Avalonia build registers the same `ShaPrint.Platform.Windows` adapters
(Spooler, WIA scanner, virtual printer + `DriverSafetyGuard`, toast notifications,
firewall, startup).

### Windows troubleshooting

- **Client cannot find the server:** open UDP 9876 and TCP 9877 on the server's Windows
  Firewall (the app tries to add these rules on server start, with a UAC prompt). On a
  different subnet use **Specific Server IP** instead of auto-discovery.
- **"Driver X is not installed":** install the official manufacturer driver on the client
  before installing the virtual printer.

---

## macOS & Linux

> v1 is **CLI-first**: everything talks to CUPS/SANE through command-line tools
> (`lpstat`, `lp`, `lpadmin`, `lpinfo`, `scanimage`) — no P/Invoke into libcups/libsane.
> **None of the runtime behaviors below have been verified on real macOS/Linux hardware
> yet** (see [Not verified](#not-verified--future-work)); they reflect the implemented
> code paths and are exercised by CI builds and contract tests only.

### Prerequisites

| Tool | macOS | Linux |
|------|-------|-------|
| CUPS | Built-in | `sudo apt install cups` (Debian/Ubuntu) or distro equivalent |
| SANE scanner | `brew install sane-backends` | `sudo apt install sane-utils` (or distro equivalent) |
| .NET 8 runtime/SDK | `brew install --cask dotnet-sdk` or from dot.net | distro packages or dot.net |

Verify the scanner backend before using Scan mode:

```bash
scanimage -L            # list detected scanners; empty output = backend/driver issue
```

> The macOS scanner path deliberately uses SANE (`sane-backends`), **not** the macOS
> ImageCapture framework — ImageCapture is deferred to Future Work. Vendor scanner
> drivers (e.g. Epson/Canon `scanimage` backends) are used if present, but there is no
> macOS ImageCapture integration in v1.

### Build & run the Avalonia UI

```bash
# plain net8.0 TFM (macOS + Linux; no Windows-only references)
dotnet build ShaPrint.UI/ShaPrint.UI.csproj -f net8.0
dotnet run  --project ShaPrint.UI/ShaPrint.UI.csproj -f net8.0
```

### CLI sender (`shaprint send`)

The CLI sender is the same `ShaPrint.UI` binary run headlessly. From source:

```bash
dotnet run --project ShaPrint.UI/ShaPrint.UI.csproj -f net8.0 -- send --printer "PrinterName" --file /path/to/file.pdf [--host 192.168.1.50]
```

For an installed machine, publish once and expose it as `shaprint` on `PATH` (the CUPS
backend looks for `shaprint` on `PATH`, then `/usr/local/bin/shaprint`, then
`$HOME/.local/bin/shaprint`, or honors the `SHAPRINT_CLI` environment variable):

```bash
# framework-dependent (needs .NET 8 runtime on the machine)
dotnet publish ShaPrint.UI/ShaPrint.UI.csproj -f net8.0 -c Release -o /usr/local/lib/shaprint
sudo ln -sf /usr/local/lib/shaprint/ShaPrint.UI /usr/local/bin/shaprint

# or self-contained (no .NET runtime needed)
dotnet publish ShaPrint.UI/ShaPrint.UI.csproj -f net8.0 -c Release -r linux-x64 --self-contained true -o /usr/local/lib/shaprint
sudo ln -sf /usr/local/lib/shaprint/ShaPrint.UI /usr/local/bin/shaprint

# verify
shaprint send --printer "PrinterName" --file /tmp/job.pdf    # exit codes: 0 ok / 1 usage / 2 send failed
```

See [CLI reference](#cli-reference) for the full contract.

### Virtual printer on macOS/Linux (CUPS backend, needs root)

A ShaPrint virtual printer is a CUPS queue whose device URI points at the ShaPrint
backend: `shaprint://sha/<url-encoded printer name>`. Installing it writes **root-owned
system files**:

| Component | Path (varies by distro) |
|---|---|
| CUPS backend script (executable) | `/usr/lib/cups/backend/shaprint` (Debian/Ubuntu, openSUSE) · `/usr/libexec/cups/backend` (macOS, RHEL/Fedora) · `/usr/lib64/cups/backend` (RHEL/Fedora x64) |
| Raw passthrough PPD | `/usr/share/cups/model/shaprint/shaprint.ppd` |

The app does **not** assume `File.WriteAllText` succeeds on those paths. When the write
fails (or `lpadmin` reports a permission error), it prints ready-to-run instructions, e.g.:

```bash
sudo install -m 755 '<staged-backend-file>' '/usr/lib/cups/backend/shaprint'
# or: pkexec install -m 755 '<staged-backend-file>' '/usr/lib/cups/backend/shaprint'
sudo lpadmin -p "PrinterName" -v "shaprint://sha/PrinterName" -P '/usr/share/cups/model/shaprint/shaprint.ppd' -E
```

(`PrivilegeEscalationHelper` stages the content in a temp file the user can reach, then
hands over the exact `sudo`/`pkexec` command. When ShaPrint is packaged, the package
`postinst` hook should perform these writes instead.) Removal uses `lpadmin -x`; the same
sudo/pkexec flow applies.

> The app auto-detects the backend directory (`/usr/lib/cups/backend` vs
> `/usr/libexec/cups/backend` vs `/usr/lib64/cups/backend`) at runtime.

### Startup at login

- **macOS:** a LaunchAgent plist at `~/Library/LaunchAgents/com.shaprint.app.plist`,
  registered with `launchctl load -w` / `launchctl unload -w`.
- **Linux:** a systemd user unit at `~/.config/systemd/user/shaprint.service`, e.g.
  `systemctl --user enable --now shaprint.service`.

### Notifications

- **macOS:** `osascript -e 'display notification ...'`.
- **Linux:** `notify-send <title> <body>` (libnotify).
- Toast click actions (`ToastAction`) have no activation semantics on these channels and
  are accepted but ignored in v1.

### Firewall (best-effort, may need root)

- **macOS:** the app registers itself with the application firewall via
  `socketfilterfw --add` / `--unblockapp`; if that needs root it prints
  `sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add /path/to/ShaPrint`.
- **Linux:** detects `ufw`, then `firewall-cmd`, then `iptables` and opens UDP 9876 /
  TCP 9877 / TCP 9878. If no supported tool is found (or the tool needs root), it logs
  and continues — open the ports with your distro's tool manually.

### Scanner (SANE `scanimage`)

- Enumeration: `scanimage -L` (parsed in-process; per-backend entries listed).
- Scan: `scanimage -d <scanner-id> --format=<fmt> --mode=<mode> --resolution=<dpi>` with
  the binary output read via `RedirectStandardOutput` and written to a file by the app
  (shell redirect `>` is never used in the process arguments).

### CLI sender end-to-end example (macOS/Linux)

```bash
# 1. Install the virtual printer (see "Virtual printer" above)
# 2. Print from any app to the "ShaPrint ..." queue
#    OR send a file directly:
shaprint send --printer "Epson L3210" --file ~/Documents/report.pdf

# 3. Same machine's backend script path (automatic when printing from an app):
#    CUPS -> /usr/lib/cups/backend/shaprint -> shaprint send -> TCP 9877 -> server
```

---

## Android

The Android app is a **print-only client** (no virtual printer, no scanner, no monitor).
It discovers ShaPrint servers over UDP, lets you pick a printer, and relays a file you
pick via the Storage Access Framework (`ACTION_OPEN_DOCUMENT`) — PDF and images.

### Build the Release APK

```bash
dotnet build ShaPrint.Android/ShaPrint.Android.csproj -f net8.0-android -c Release
```

Output APK (signed; 4 ABIs: arm64-v8a, armeabi-v7a, x86, x86_64):

```
ShaPrint.Android/bin/Release/net8.0-android/com.shaprint.android-Signed.apk   (~44 MB)
```

Release builds use trimming + profiled AOT; `FixAvaloniaCompileOutputPath` in
`ShaPrint.Android.csproj` normalizes the Avalonia XAML compile path so the IL1032
"root assembly not found" failure no longer occurs (do not reintroduce older workarounds).

### Install

```bash
adb install -r ShaPrint.Android/bin/Release/net8.0-android/com.shaprint.android-Signed.apk
```

**Debug builds:** `adb install` of a Debug APK aborts with
*"No assemblies found ... Fast Deployment"*. To deploy Debug, either install the Release
APK (above) or use the fast-deployment path:

```bash
dotnet build ShaPrint.Android/ShaPrint.Android.csproj -t:Install -f net8.0-android
```

### Android limitations (v1)

- **Discovery needs multicast:** works on normal home/office Wi-Fi (a
  `WifiManager.MulticastLock` is held for the scan duration). **AP-isolated / client
 -isolation Wi-Fi cannot discover servers** — the hotspot/router blocks the multicast.
- **No unicast sweep:** unlike the desktop client, Android skips the per-interface
  unicast IP sweep (it would cost ~1024 socket sends per scan); discovery relies on
  subnet-directed broadcasts.
- **No network channel UI / no notifications in v1:** the manifest carries
  `POST_NOTIFICATIONS`, but it is **not requested at runtime** and no notifications are
  posted yet (future work).
- 5 permissions total (`INTERNET`, `ACCESS_NETWORK_STATE`, `ACCESS_WIFI_STATE`,
  `CHANGE_WIFI_MULTICAST_STATE`, `POST_NOTIFICATIONS`); no storage permissions — file
  access is per-file via SAF.

---

## CLI Reference (`shaprint send`)

Handled by `ShaPrint.UI/Cli/CliDispatcher.cs` **before** the GUI starts — the GUI never
launches on the CLI path. Runs on Windows, macOS and Linux.

```
Usage: shaprint send --printer <name> --file <path> [--host <ip>]

  --printer <name>   Target printer name exposed by the ShaPrint server (required).
  --file <path>      Path to the job file (required). Empty files are refused.
  --host <ip>        Optional server IP. When omitted, the server is discovered on the
                     LAN via UDP 9876 (HMAC-verified).
```

- The verb is case-insensitive (`SEND`, `Send` all work).
- Both `--printer X` and `--printer=X` forms are accepted.
- Exit codes: **0** job relayed successfully · **1** argument/usage or file-IO error ·
  **2** send attempt failed (no server found, connection, discovery or authentication).

Example:

```bash
dotnet run --project ShaPrint.UI/ShaPrint.UI.csproj -f net8.0 -- send --printer "Epson L3210" --file /tmp/job.pdf
echo $?   # 0 = accepted by the server
```

---

## Troubleshooting

| Symptom | Check |
|---|---|
| Discovery finds no servers | Ports 9876/UDP + 9877/TCP open on the server; clients share the same **Network Channel**; on different subnets use `--host <ip>` / Specific Server IP. Android: AP-isolated Wi-Fi blocks multicast — no discovery by design. |
| `shaprint send` exits 2 | No server reachable. Run with `--host <ip>` to bypass discovery; confirm the Network Channel matches; check the server's firewall. |
| Firewall blocks the relay | Open UDP 9876, TCP 9877, TCP 9878 (server). macOS: `socketfilterfw` registration; Linux: `ufw allow 9876/udp`, `ufw allow 9877/tcp`, `ufw allow 9878/tcp` (or `firewall-cmd`/`iptables` equivalent). |
| `scanimage -L` returns nothing | The SANE backend for your scanner is missing/not installed: macOS `brew install sane-backends`; Linux `sane-utils` (and vendor backend packages). Reboot or re-plug the scanner after install. |
| CUPS job fails with "shaprint send failed" | `shaprint` CLI not on PATH — install it (`sudo ln -sf ... /usr/local/bin/shaprint`) or set `SHAPRINT_CLI` to its path. |
| Virtual printer install fails | Backend/PPD writes need root: run the printed `sudo install ...` / `sudo lpadmin ...` commands (see Virtual printer section). |
| Android Debug install aborts "No assemblies found" | Use `dotnet build -t:Install` (fast deployment) or install the Release APK. |
| Print fidelity wrong (all platforms) | The client queue must use the same driver as the physical printer on the server (Windows). On macOS/Linux v1 the raw PPD passes bytes through — the server renders. |

---

## CI

`.github/workflows/ci.yml` runs on every push/PR:

| Runner | Verifies |
|---|---|
| `windows-latest` | Builds `ShaPrint.ci.slnf` (all projects except Android), runs the **full 348-test suite** (`ShaPrint.Tests`) and the 11 os-agnostic contract tests. |
| `ubuntu-latest`, `macos-latest` | Cross-OS build of the solution filter (`EnableWindowsTargeting`), runs the **11 contract tests** (`ShaPrint.Tests.Contract`) — the Windows-TFM suite cannot execute off-Windows. |
| `ubuntu-latest` (android job) | `dotnet workload install android`, then builds the Release APK and uploads it as an artifact. |

Contract tests pin the cross-platform surface: `IVirtualPrinterManager` has **no**
`pipeName` in its signature, `IPrintRelayClient.SendAsync` validates `PrintJobPayload`
serialization against `Constants`, and `ServerStatusPayload` keeps the shape the Monitor
UI binds to.

---

## Not Verified / Future Work

Honest list of what is implemented but **not yet proven on real hardware**, and what is
deferred:

- **macOS/Linux runtime on real machines** — `lpadmin` virtual-printer install,
  per-backend `scanimage` behavior, `osascript`/`notify-send`, `launchctl`/`systemd`,
  `sudo`/`pkexec` escalation, and firewall tools have been implemented and CI-built but
  **not executed on physical macOS/Linux hardware**. Treat v1 Unix behavior as
  "implemented, pending hardware verification".
- **macOS ImageCapture framework** — v1 uses SANE (`scanimage`); ImageCapture is future
  work (deferred in the design spec).
- **P/Invoke libcups/libsane** — can be revisited after the CLI-first path is stable
  (mind the historical `cups_dest_t` marshaling bug, stride 32 vs 40 bytes).
- **Android** — verified as a build (APK) and on emulator-free CI only; on-device
  multicast behavior, `POST_NOTIFICATIONS` runtime request, and per-ABI testing are
  future work. The Android build ships 4 ABIs by default; a subset (e.g. `arm64-v8a`
  only) could shrink the ~44 MB APK further.
- **`ISystemIntegration`** — a formal abstraction for OS-level integration beyond
  printer/scanner/notification/firewall/startup/relay is deferred (spec Future Work)
  until a concrete need appears.
- **Driver-specific PPD selection on Unix** — v1 always uses the raw passthrough PPD.
- **Auto-update self-cycle on macOS/Linux** — the shared `UpdateService` launches a
  per-OS `ShaPrint.Updater`; the full check → download → replace → relaunch cycle has not
  been run on a real Mac/Linux box.
- **Web upload-to-print** — out of scope; will be specified separately.
- **iOS** — out of scope.
