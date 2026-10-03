# Desktop auto-updater, job-aware restart, and persistent server sharing

---
status: accepted
supersedes: parts of ADR 0006 (server sharing startup)
extends: ADR 0004 (discovery advertisements)
---

ShaPrint updates itself on the fly using `tauri-plugin-updater` and signed GitHub Releases bundles, protects in-flight print jobs with an active-job drain guard before restarting, persists server sharing preferences so shared printers survive restarts, and advertises application versions over multicast DNS discovery.

## Why it works this way

### 1. Unprivileged per-user updates via official Tauri plugin
Routine desktop app updates must not demand Windows UAC administrator elevation (ADR 0002). Configuring the Windows NSIS installer for `currentUser` places the binaries in the user profile directory (`%LOCALAPPDATA%`), allowing `tauri-plugin-updater` to download, verify with Minisign, and replace the application bundle on the fly. Administrator elevation remains strictly limited to inbound firewall rules and native client queue creation (ADR 0005).

### 2. Job-aware drain guard before restart
Neither the client proxy nor the server IPPS endpoint can tolerate an abrupt process termination while an IPP/IPPS print job stream or spooler handoff is active. An in-flight job tracker maintains an atomic count of active streaming print jobs across both runtimes. Any update restart request is held until `active_jobs == 0` and an 8-second drain cooldown window elapses, allowing the Windows Print Spooler to finish buffering and spooling data before the process shuts down.

### 3. Idle tray auto-apply vs. interactive UI prompt
ShaPrint is designed to live in the notification area for the entire signed-in user session (ADR 0006). For an unwindowed app sitting in the system tray, waiting for manual user interaction would leave background installations unpatched. When the main window is hidden and no print jobs have run for 15 minutes, ShaPrint automatically applies the update and restarts cleanly. When the main window is visible, ShaPrint displays a non-intrusive banner and tray menu item, letting the user trigger the restart when convenient.

### 4. Persistent server sharing across restarts (superseding ADR 0006)
ADR 0006 originally kept server sharing strictly manual at every launch, meaning sharing never started on its own. In production office networks, workstations acting as print servers frequently reboot after Windows updates or power cycles; requiring an administrator to log in, launch the UI, select printers, and click "Start Sharing" led to network outages for connected clients.
Under this decision, the server's selected shared printers and sharing status (`is_sharing_enabled`) are stored in `app_data_dir/sharing_state.json`. At startup (Windows login, manual launch, or post-update relaunch), ShaPrint validates the saved printers against the currently available local print queues; if sharing was previously enabled and at least one selected queue remains valid, server sharing starts automatically alongside the client proxy.

### 5. Multicast DNS version advertisement (extending ADR 0004)
Multicast DNS advertisements for `_shaprint-ipps._tcp.local.` carry a `v=<semver>` TXT record attribute. Client discovery displays an advisory badge when a nearby server runs a newer or mismatched version, giving visibility into LAN version skew without disrupting IPPS print submissions.

## Consequences

- Releases on GitHub require `TAURI_SIGNING_PRIVATE_KEY` in GitHub Actions secrets to generate `latest.json` and cryptographic signatures.
- `Sharing` state changes and explicit service starts/stops write to disk, ensuring consistency across unexpected power losses.
- `ClientProxyService` and `IppsServer` must acquire a job lease during `Print-Job` processing to register in-flight activity with the drain guard.
- Backward compatibility: clients discovering servers running older ShaPrint versions without a `v=` TXT attribute handle `version: None` gracefully.
