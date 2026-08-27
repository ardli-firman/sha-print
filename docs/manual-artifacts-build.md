# ShaPrint Manual Artifact Builds (multi-platform)

> Manual, on-demand pipeline that produces distributable artifacts for the
> multi-platform app (`ShaPrint.UI` Avalonia) and the Android client. It complements —
> but never touches — the automated **beta/stable release workflows**, which keep
> producing the Windows **WPF + Updater Inno Setup installer** on push to `develop`/`main`.
>
> Companion doc: [multi-platform-guide.md](./multi-platform-guide.md) covers the app
> itself; this page covers **how to produce and publish the artifacts**.

---

## Purpose & scope

The scripts under `scripts/` and the manual workflow `.github/workflows/release-artifacts.yml`
produce **one artifact per platform** from the current checkout:

| Platform | Artifact | Contents |
|---|---|---|
| Windows x64 | `shaprint-win-x64-v<version>.zip` | `ShaPrint.UI.exe` — self-contained single-file Avalonia app (net8.0-windows10.0.17763) |
| macOS x64 | `shaprint-macos-x64-v<version>.zip` | `ShaPrint.UI` — self-contained single-file (net8.0, osx-x64) |
| macOS arm64 | `shaprint-macos-arm64-v<version>.zip` | `ShaPrint.UI` — self-contained single-file (net8.0, osx-arm64) |
| Linux x64 | `shaprint-linux-x64-v<version>.tar.gz` | `ShaPrint.UI` — self-contained single-file (net8.0, linux-x64) |
| Android | `shaprint-android-v<version>.apk` | `com.shaprint.android-Signed.apk` (~44 MB, 4 ABIs, trimmed + profiled AOT) |

All artifacts are staged into `artifacts/<version>/` (gitignored) and the scripts print
the **absolute output paths**.

Caveats:

- **Nothing is signed / notarized** (except the Android debug-keystore signature, see
  [Android signing note](#android-signing-note)). Windows will show SmartScreen, macOS
  Gatekeeper will quarantine — treat these as **internal/testing builds** until a proper
  signing pipeline exists (see [Not verified yet](#not-verified-yet)).
- The Windows **WPF** app + **Updater installer** is still produced exclusively by the
  existing **beta/stable release workflows** — this manual pipeline does not build it.
- These artifacts are **not** wired into the auto-update flow.

---

## Prerequisites per platform

| Platform | Requirements |
|---|---|
| All | .NET **8.0.x SDK** (repo `global.json` pins `8.0.203`). `git` on PATH (for the default version). The scripts locate `dotnet` via PATH **first** and fall back to the default install locations (`C:\Program Files\dotnet` on Windows; `/usr/local/share/dotnet`, `~/.dotnet` on macOS/Linux), so a PATH entry without an SDK (e.g. an app-bundled `dotnet`) is tolerated. |
| Windows | Windows host. Optional: nothing else — the `win-x64` self-contained build needs no installed .NET runtime on the target machine. |
| macOS | A **macOS** host (`scripts/publish-macos.sh` must run on macOS). `zip` (preinstalled). |
| Linux | A **Linux** host. `tar` (preinstalled). |
| Android | The **Android workload** + Android SDK + JDK 21 (any host): |

```bash
dotnet workload install android
```

Then set, per your shell:

```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"        # macOS
export ANDROID_HOME="$HOME/Android/Sdk"                # Linux
export ANDROID_HOME="C:\Users\<you>\AppData\Local\Android\Sdk"   # Windows
export JAVA_HOME="C:\Program Files\Java\jdk-21"        # Windows (or your JDK 21 path)
```

The Android script **honors** `$env:ANDROID_HOME` / `$env:JAVA_HOME` when set and falls
back to .NET Android's default SDK location otherwise. On GitHub runners both are
preinstalled (`ANDROID_HOME=/usr/local/lib/android/sdk`).

---

## Manual commands per platform

Each script takes an optional version as its first argument (`-Version` on Windows).
When omitted, the version is `git describe --tags --always` (leading `v` stripped —
NuGet rejects `v1.2.3` as a `-p:Version`), falling back to a **dotted** date+short-sha
(`2026.08.27-abc1234` — NuGet requires `Major.Minor...`), sanitized to `[A-Za-z0-9._-]`
and validated as `X.Y[.Z[.R]][-suffix]`. Run the scripts **from the repo root** (or
anywhere — they resolve the repo root themselves).

### Windows (exe zip)

```powershell
# PowerShell 5.1+ / pwsh
.\scripts\publish-windows.ps1 -Version 1.0.0-test
# or let it derive the version:
.\scripts\publish-windows.ps1
```

Expected output:

```
==> Publishing ShaPrint.UI for win-x64 (version: 1.0.0-test)
...
==> Artifact ready: D:\...\shaprint\artifacts\1.0.0-test\shaprint-win-x64-v1.0.0-test.zip
```

The zip contains a single self-contained `ShaPrint.UI.exe` (Avalonia UI — also the
`shaprint send` CLI on Windows).

### macOS (x64 + arm64 zips)

```bash
./scripts/publish-macos.sh 1.0.0-test
```

Builds **both** `osx-x64` and `osx-arm64`, producing:

```
artifacts/1.0.0-test/shaprint-macos-x64-v1.0.0-test.zip
artifacts/1.0.0-test/shaprint-macos-arm64-v1.0.0-test.zip
```

The script prints a reminder that the zips are **not codesigned/notarized** — see
[macOS / Linux caveats](#macos--linux-caveats).

### Linux (x64 tar.gz)

```bash
./scripts/publish-linux.sh 1.0.0-test
```

Produces:

```
artifacts/1.0.0-test/shaprint-linux-x64-v1.0.0-test.tar.gz
```

### Android (APK)

```powershell
# Windows host (or any host with the android workload + SDK + JDK)
.\scripts\publish-android.ps1 -Version 1.0.0-test
```

Steps performed: (1) `-t:InstallAndroidDependencies -p:AcceptAndroidSdkLicenses=true`
to install missing SDK components non-interactively, (2) a normal Release build
(trimming + profiled AOT), (3) copy of the signed APK:

```
artifacts/1.0.0-test/shaprint-android-v1.0.0-test.apk
```

If the APK is missing after the build the script fails with a clear error pointing at
`ShaPrint.Android/bin/Release/net8.0-android/com.shaprint.android-Signed.apk`.

> Note: the APK keeps the app version baked into the csproj (`<Version>2.0.0</Version>`);
> the artifact **file name / folder** use the script's version.

---

## GitHub Actions manual run

The `Multi-Platform Artifacts` workflow is **manual-only** (`workflow_dispatch`) — it
never runs on push/PR. To trigger it:

1. Open **Actions** → **Multi-Platform Artifacts** → **Run workflow**.
2. Fill the inputs:

| Input | Meaning |
|---|---|
| `version` | Artifact version. **Empty** = auto `YYYY.MM.DD-<short-sha>` (same in all jobs; dotted date because NuGet requires `Major.Minor...`). Provide e.g. `1.0.0-test` to pin it. |
| `release_mode` | `draft` (default) = create a **Draft** GitHub Release holding all four artifacts; `tag` = create a **published** Release + tag `v<version>-artifacts`; `none` = only upload workflow artifacts, no GitHub Release. |

3. Click **Run workflow**.

All four platform jobs run in parallel (Windows / macOS / Linux / Android). The
`release` job then downloads the merged artifacts and (unless `release_mode: none`)
creates the GitHub Release:

- `draft` → a **draft** release at tag `v<version>-artifacts`, `prerelease: true`. The
  tag is created when you publish the draft. Nothing is public until you do.
- `tag` → a published prerelease; the tag `v<version>-artifacts` is created immediately.

You can also download the per-platform zips/apk from the workflow run page (Artifacts
section) without any GitHub Release.

---

## Android signing note

By default the Release APK is signed with the **debug keystore**
(`~/.android/debug.keystore`) — fine for internal testing, **not** for distribution.
To sign with a real keystore, pass the standard .NET Android signing properties to the
build (e.g. to `scripts/publish-android.ps1`'s `dotnet build` step, or directly):

```powershell
dotnet build ShaPrint.Android/ShaPrint.Android.csproj -f net8.0-android -c Release `
  -p:AndroidSigningKeyStore=C:\keys\shaprint.keystore `
  -p:AndroidSigningKeyAlias=shaprint `
  -p:AndroidSigningKeyPass=*** `
  -p:AndroidSigningStorePass=***
```

When these are set, the produced APK is signed with the release key instead of the
debug key. (Automating real signing via CI secrets is future work — see below.)

---

## macOS / Linux caveats

**macOS — Gatekeeper quarantine.** Downloads are tagged with a quarantine attribute, so
the first launch shows *"ShaPrint" cannot be opened because the developer cannot be
verified*. Remove the quarantine on the unzipped binary (no admin needed):

```bash
xattr -dr com.apple.quarantine /path/to/ShaPrint.app-or-binary
# or right-click → Open → Open once
```

Full codesigning + notarization would remove this entirely (future work).

**Linux — runtime libraries.** The self-contained single-file binary still needs the
distro's GUI libraries (Avalonia/SkiaSharp load them at runtime). On Debian/Ubuntu, if
the app fails to start, install:

```bash
sudo apt install libx11-6 libxrandr2 libxcursor1 libxinerama1 libxi6 libgl1 libegl1 libfontconfig1
```

(`libX11` / `libXcursor` are the usual culprits; other distros have equivalent packages.)

**Both** — the binaries expect a working CUPS/SANE setup for print/scan (see
multi-platform-guide.md); the CLI sender (`shaprint send`) works headless.

---

## Not verified yet

Honest list of what this pipeline deliberately does **not** do yet:

- **macOS codesigning + notarization** — artifacts are unsigned; Gatekeeper quarantine
  must be removed manually.
- **Windows Authenticode signing** (SmartScreen will warn).
- **Android release signing via CI** — still the debug keystore unless signing props are
  passed manually.
- **`.deb` / `.rpm` / `.AppImage` packaging** for Linux.
- **macOS universal binary** (`osx-universal`) — x64 and arm64 are shipped as two zips.
- **Auto-update integration** for the multi-platform artifacts.

All of these are future work; the manual pipeline is intentionally minimal so each piece
can be added on top without reworking the flow.
