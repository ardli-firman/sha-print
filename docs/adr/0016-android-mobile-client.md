# Android mobile client architecture

---
status: accepted
---

ShaPrint will provide an Android client-only application (`apps/android`, application ID `io.shaprint.client`) that connects directly to ShaPrint servers over IPPS. Unlike the Windows desktop client, the Android client runs without a loopback proxy or desktop print spooler, communicating directly with the server endpoint over TLS port 48631.

The mobile client adheres to the existing wire protocol (RFC 8010/8011 IPPS and ADR 0010): it performs client-side rendering to generate `image/pwg-raster` at 300 dpi (`srgb_8` and `sgray_8`) with PWG 5102.4 page headers and PackBits run-length encoding. PDFs are rendered page-by-page using Android's native `PdfRenderer`, and images are rendered directly to a 300 dpi canvas with user-selected fit or fill page layout. Pages are encoded and streamed directly into the HTTP/TLS request body so device memory consumption remains bounded to a single uncompressed page (~35 MiB) regardless of document length.

Discovery uses on-demand multicast DNS queries (UDP 48633, `_shaprint-ipps._tcp.local.`) guarded by `WifiManager.MulticastLock` with a 10–15 second timeout to preserve battery life, supplemented by manual host:port entry for cross-VLAN servers. Server trust follows the Trust-On-First-Use (TOFU) model: nearby servers display their SHA-256 certificate fingerprint for explicit user approval before becoming trusted servers. Approved fingerprints and Network Channel credentials are encrypted using hardware-backed Android Keystore and `EncryptedSharedPreferences`. Changed certificate identities block subsequent queries and print jobs until re-approved.

Document ingestion supports both an Android `PrintService` plugin (surfacing shared printers from trusted servers in the system print dialog) and a standalone application with an in-app file picker and system Share Sheet target. Active print transmissions run inside an Android Foreground Service with real-time notification progress and mid-job cancellation (closing the socket and dispatching IPP `Cancel-Job`).

The app targets modern Android standards: `minSdk = 26` (Android 8.0) and `targetSdk = 35` / `compileSdk = 35` (Google Play Store compliance through late 2026).

## Consequences

- The Android client introduces zero breaking changes: existing Windows/Linux servers accept print jobs without modification or new wire formats.
- ShaPrint server code remains unchanged; no PDF renderer or third-party decoding libraries are added to the server binary.
- Cross-VLAN servers remain reachable via manual host:port entry with persistent fingerprint verification.
- Unit tests cross-validate Android IPP serialization and PWG raster output against the existing Rust desktop test vectors in `apps/desktop/src-tauri/tests`.
