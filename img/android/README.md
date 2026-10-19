# Android screenshots

These PNGs are unedited captures from ShaPrint running on an Android 16 (API 36)
emulator, taken on 2026-10-17 with `adb exec-out screencap -p`. They replace the
four generated screenshot illustrations formerly in `img/`.

- App: `io.shaprint.client`, version 1.0.3, versionCode 4, debug build.
- Source revision: `697ad4e3c092638fa8c7ee9e3f97801fdd13e0e8`; Android source was unchanged.
- UI: Kotlin / Jetpack Compose. This repository's Android app is not Flutter.
- Emulator: `test_avd`, Android 16 Google APIs x86_64, 1080 × 1920, density 420.
- State: no trusted servers or shared printers. No server or print job is fabricated.
- Screens: Shared Printers, Network Discovery, Trusted Servers.
- Runtime exercised: cold launch, tab navigation, start network scan; the real scan
  showed “Scanning local Wi-Fi...” and “Searching your Wi-Fi...”. No real print
  submission or successful discovery is claimed.

Build and launch from repository root:

```bash
cd apps/android
./gradlew :app:assembleDebug --console=plain
adb -e install -r app/build/outputs/apk/debug/app-debug.apk
adb -e shell am start -W -n io.shaprint.client/.MainActivity
```

Navigate to each tab in the running app, wait for its UI to settle, then capture
from repository root. Do not replace UI with drawings or generated mockups.

```bash
adb -e exec-out screencap -p > img/android/android-printers.png
adb -e exec-out screencap -p > img/android/android-discovery.png
adb -e exec-out screencap -p > img/android/android-servers.png
```

The landing build copies these PNGs byte-for-byte into `/assets/`. The hero scales
one capture inside a decorative phone frame; the gallery links to the full PNGs.
No synthetic buttons or UI labels are overlaid on the captured screen.

Capture provenance (SHA-256):

```
4dbd340ac5be11b29f22a8405831f13e58b53e228d9b71bb825db5f6ac3709b2  apps/android/app/build/outputs/apk/debug/app-debug.apk
319cf7b5f197a441195194e1c2d1527e3f908030624c8e4ca4c16f26fe0574b3  img/android/android-discovery.png
a999ddb5d0c3cfa7de84f0c0d07e15bd613e507ba014a15be5d479ab1a385c53  img/android/android-printers.png
46394f151ba9f54e56848e01c7dbfaa8314d476270c4a42a0da0936da4bf7b4a  img/android/android-servers.png
```
