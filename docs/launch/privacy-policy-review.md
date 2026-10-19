# Privacy policy source review and Play Store handoff

Reviewed on 19 October 2026. The public policy source is
`apps/landing/public/privacy/index.html`, intended URL
`https://sha-print.pages.dev/privacy/`. The user explicitly approved publishing
`ardlifirman17@gmail.com` as the privacy contact. The policy names ShaPrint and
Android package `io.shaprint.client` and separately explains website processing.

## Implementation evidence

This is a source review of the current Android code, not a new full runtime
privacy audit or a guarantee of Play approval. The codebase-memory transport was
unavailable; the relevant source was read directly and scoped searches were used.

| Policy topic | Source checked and resulting disclosure |
| --- | --- |
| Files and cached copies | `ui/navigation/ShaPrintNavHost.kt` uses Android OpenDocument, retains selected URI permission, and caches a chosen document. Older selected-document caches are removed when another document is selected; not every cache is immediately erased after printing. `features/print/DocumentPreviewHelper.kt` also prepares a local preview cache. |
| Transmission | `core/network/IppsClient.kt` submits directly to the selected server. `core/ipp/IppMessage.kt` includes print job name, settings, and Network Channel when configured. No claim that no data leaves the phone, or that a manually entered address must be within local Wi-Fi. |
| Processing and recipients | `features/printservice/ShaPrintSystemPrintService.kt` copies Android spooler data into cache and removes its temporary copy after processing. The receiving server, its operating system, spooler, and printer can retain their own records independently. |
| Notification visibility | `features/print/PrintJobForegroundService.kt` includes the job and printer names in notifications. Disclosure notes device notification/lock-screen settings. |
| Trust information | `core/security/EncryptedTrustStore.kt` stores server addresses, names, fingerprints, and connection information in app-private preferences. Only the Network Channel value is encrypted through `KeystoreCrypto.kt`; do not claim all server metadata is encrypted with Keystore. Removing a server removes its stored preference entry. |
| Backup | `AndroidManifest.xml` has `android:allowBackup="true"`, without custom exclusion rules. Eligible preferences may participate in Android backup or device transfer. Standard Auto Backup excludes cache directories. Policy does not promise that deleting this installation deletes external backups. |
| Permissions and SDKs | `AndroidManifest.xml`, `app/build.gradle.kts`, and scoped searches of the main source show network, multicast, notifications, foreground processing, and print-service integration; no account, ads, analytics SDK, or GPS/contact/camera/microphone access in this reviewed implementation. |
| Website measurements | `apps/landing/src/site.ts`, `functions/api/download.ts`, and `migrations/0001_download_counts.sql` report allowlisted package clicks to daily aggregate counts, honoring DNT/GPC. No visitor identifier is stored in that table. There is no aggregate auto-expiry job. Provider HTTP request processing is disclosed separately. |

Android file paths in this table are under
`apps/android/app/src/main/java/io/shaprint/client/`, except the manifest and
Gradle file under `apps/android/app/`.

## Store-listing checks

The page now has an explicit privacy-policy title, scope/app identity, privacy
contact, data categories and purposes, recipients, security, retention/deletion,
backup, permissions, support correspondence, website-provider links, and update
date. It is static HTML readable without JavaScript or an account.

Before submitting the listing:

- Publish manually with `make landing-deploy`, then verify the exact HTTPS URL
  resolves publicly and serves the policy without login or geographic restrictions.
  This session's public DNS lookup could not resolve `sha-print.pages.dev`; no
  production publication or public availability is claimed.
- Add the same privacy-policy link or policy text within the Android app. The
  current UI has no such link; this website change does not modify Android code.
- Review the Play Console Data safety answers for the exact submitted APK/AAB,
  including user-directed server transfers and Android backup. Website-only
  analytics must not be described as an in-app analytics SDK. This work does not
  submit the form, assert that every off-device transfer is exempt, or claim Play
  approval.
- Review policy and declarations again if SDKs, destinations, permissions, backup,
  document retention, support practices, or business practices change.

## Primary references

- [Google Play User Data policy](https://support.google.com/googleplay/android-developer/answer/10144311?hl=en): app identification, data handling, privacy contact, retention/deletion, public HTML URL, and in-app policy access.
- [Google Play Data safety guidance](https://support.google.com/googleplay/android-developer/answer/10787469?hl=en): declarations must match the submitted app, its transfers, and included SDKs.
- [Android Auto Backup](https://developer.android.com/identity/data/autobackup): eligible preferences, excluded cache directories, and system-managed backup/restore.
- [Cloudflare privacy policy](https://www.cloudflare.com/privacypolicy/) and [GitHub privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement): external website hosting and download providers.

## Verification observed

`make landing-build` succeeded. `bun run typecheck && bun run test` passed both
workspace typechecks, 84 desktop tests, and 19 landing tests. Chrome on the local
Pages runtime served `/privacy/` with HTTP 200 and verified the title, app identity,
contact email, date, canonical URL, section anchors, and landing footer link.
Seven widths from 320 to 1440 px had no horizontal overflow. axe found zero
violations at 1440 and 390 px; the page remained readable without JavaScript, and
no page exceptions were observed. Desktop and narrow screenshots were inspected.

No breaking changes. Stable clients can still print to Nightly servers and Nightly
clients to Stable servers under the same supported-version assumptions. Settings,
Trusted Servers, shared printer selections, and Network Channel storage remain
backward-compatible when downgrading. No app code, wire protocol, settings schema,
or backup behavior was changed by this policy update.
