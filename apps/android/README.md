# ShaPrint Android Client

The ShaPrint Android Client (`io.shaprint.client`) connects to nearby or trusted ShaPrint servers over standard IPPS and multicast DNS (mDNS) discovery, enabling Android devices to discover shared printers and submit print jobs directly or via the Android Print Spooler framework.

## Play Store Readiness

- **Package Name (Application ID)**: `io.shaprint.client`
- **Target SDK**: `36` (Android 16)
- **Min SDK**: `26` (Android 8.0 Oreo)
- **Bundle Format**: Android App Bundle (`.aab`) with R8 minification and resource shrinking enabled.
- **Artifact Security**: Release `.apk` files are published publicly on GitHub Releases for direct installation, while Play Store `.aab` bundles are securely uploaded as GitHub Actions Workflow Artifacts accessible only to repository maintainers.

---

## 1. Signing Configuration

### Local Development Signing
To produce signed release builds locally:
1. Generate an upload keystore if you don't have one:
   ```bash
   keytool -genkey -v -keystore release.jks -keyalg RSA -keysize 2048 -validity 10000 -alias shaprint -storetype JKS
   ```
2. Copy `key.properties.example` to `key.properties` (never commit this file):
   ```bash
   cp key.properties.example key.properties
   ```
3. Edit `key.properties` with your keystore path, alias, and passwords:
   ```properties
   storeFile=../release.jks
   storePassword=your_keystore_password
   keyAlias=shaprint
   keyPassword=your_key_password
   ```

*Note: If `key.properties` is omitted, release builds will remain unsigned without failing debug builds or unit tests.*

### CI/CD Signing (GitHub Actions)
The automated workflow `.github/workflows/release-android.yml` runs within the `release` environment.
Configure the following secrets in **GitHub Repo Settings -> Environments -> release -> Secrets**:

| Secret Name | Description |
|---|---|
| `ANDROID_KEYSTORE_BASE64` | Base64-encoded string of your `.jks` file (`base64 -w 0 release.jks`) |
| `ANDROID_KEYSTORE_PASSWORD` | Password for the keystore file |
| `ANDROID_KEY_ALIAS` | Alias name of the key (e.g. `shaprint`) |
| `ANDROID_KEY_PASSWORD` | Password for the key alias |

---

## 2. Release & Build Commands

All release steps can be executed from the repository root using `make`:

```bash
# Build Android App Bundle (.aab) for Google Play Console upload:
make build-android-release
# Output: apps/android/app/build/outputs/bundle/release/app-release.aab

# Build Universal Release APK:
make build-android-apk
# Output: apps/android/app/build/outputs/apk/release/app-release.apk

# Run Android Client unit tests:
make test-android

# Bump version, increment versionCode, commit, and create git tag:
make release-android-patch   # e.g. 1.0.0 -> 1.0.1 (tag: android-v1.0.1)
make release-android-minor   # e.g. 1.0.0 -> 1.1.0 (tag: android-v1.1.0)
make release-android-major   # e.g. 1.0.0 -> 2.0.0 (tag: android-v2.0.0)

# Push tag to trigger GitHub Actions build:
git push origin HEAD --tags
```

---

## 3. Google Play Console Compliance Checklist

When uploading the `.aab` to Google Play Console for the first time:

1. **Foreground Service Declaration (`FOREGROUND_SERVICE_DATA_SYNC`)**:
   - In **App content -> Foreground services**, check **Data sync**.
   - **User-facing reason**: *"ShaPrint streams print job data and renders PWG raster pages to local/network IPPS printers in the background to ensure jobs complete without being killed by OS battery optimizations while the user switches apps."*
   - Provide a link or short video showing a document being printed with the persistent notification visible.

2. **Android Print Service**:
   - ShaPrint implements `android.printservice.PrintService` (`ShaPrintSystemPrintService`). This integrates ShaPrint directly into Android's native system print dialog.

3. **Data Safety**:
   - The app communicates exclusively on the local network (LAN / IPPS port `48631` and mDNS port `48633`).
   - No user data, documents, or credentials are sent to external third-party cloud servers.
