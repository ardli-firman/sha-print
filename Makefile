.PHONY: help keys release-nightly release-patch release-minor release-major release-android-patch release-android-minor release-android-major test test-android build-android-debug apk-debug build-android-release build-android-apk build-rpm build-deb publish-linux-repo smoke-rpm-almalinux9 smoke-rpm-almalinux10 smoke-deb-ubuntu2204 smoke-deb-ubuntu2404 smoke-deb-debian11 smoke-deb-debian12 smoke-channel-switch typecheck check lint

help:
	@echo "Available commands in ShaPrint:"
	@echo "  make keys                  - Generate Minisign signing key pair for Tauri updater"
	@echo "  make release-nightly       - Trigger manual Nightly release from main (optional: SHA=<sha> SETTINGS_RISK=true)"
	@echo "  make release-patch         - Bump desktop patch version (x.y.Z+1), commit, and create git tag"
	@echo "  make release-minor         - Bump desktop minor version (x.Y+1.0), commit, and create git tag"
	@echo "  make release-major         - Bump desktop major version (X+1.0.0), commit, and create git tag"
	@echo "  make release-android-patch - Bump Android patch version (x.y.Z+1), increment versionCode, commit, and create git tag"
	@echo "  make release-android-minor - Bump Android minor version (x.Y+1.0), increment versionCode, commit, and create git tag"
	@echo "  make release-android-major - Bump Android major version (X+1.0.0), increment versionCode, commit, and create git tag"
	@echo "  make test                  - Run all frontend, backend, and nightly script tests"
	@echo "  make test-android          - Run Android client unit tests"
	@echo "  make build-android-debug   - Build Android client debug APK (alias: make apk-debug)"
	@echo "  make build-android-release - Build Android client release App Bundle (.aab) for Play Store"
	@echo "  make build-android-apk     - Build Android client release APK"
	@echo "  make build-rpm             - Build the AlmaLinux 9/10 headless daemon RPM artifact"
	@echo "  make build-deb             - Build the Debian/Ubuntu DEB artifact"
	@echo "  make publish-linux-repo    - Generate and sign RPM/DEB repositories (optional: CHANNEL=stable|nightly)"
	@echo "  make smoke-rpm-almalinux9  - Run the AlmaLinux 9 RPM + systemd smoke test"
	@echo "  make smoke-rpm-almalinux10 - Run the AlmaLinux 10 RPM + systemd smoke test"
	@echo "  make smoke-deb-ubuntu2204  - Run the Ubuntu 22.04 DEB + systemd smoke test"
	@echo "  make smoke-deb-ubuntu2404  - Run the Ubuntu 24.04 DEB + systemd smoke test"
	@echo "  make smoke-deb-debian11    - Run the Debian 11 DEB + systemd smoke test"
	@echo "  make smoke-deb-debian12    - Run the Debian 12 DEB + systemd smoke test"
	@echo "  make smoke-channel-switch  - Run the Stable <-> Nightly channel switch & GPG verification test"
	@echo "  make typecheck             - Run frontend TypeScript typecheck"
	@echo "  make lint                  - Run cargo fmt check and cargo clippy"

keys:
	bun run scripts/generate-keys.ts

release-nightly:
	bun run scripts/nightly/index.ts trigger --source-sha "$(SHA)" --settings-risk "$(SETTINGS_RISK)"

release-patch:
	bun run scripts/release.ts patch

release-minor:
	bun run scripts/release.ts minor

release-major:
	bun run scripts/release.ts major

release-android-patch:
	bun run scripts/release-android.ts patch

release-android-minor:
	bun run scripts/release-android.ts minor

release-android-major:
	bun run scripts/release-android.ts major

test:
	bun test scripts/nightly/
	bun run --filter shaprint-desktop test
	cd apps/desktop/src-tauri && cargo test

test-android:
	cd apps/android && ./gradlew test

build-android-debug:
	cd apps/android && ./gradlew assembleDebug
	@echo "Debug APK ready at: apps/android/app/build/outputs/apk/debug/app-debug.apk"

apk-debug: build-android-debug

build-android-release:
	cd apps/android && ./gradlew bundleRelease
	@echo "Release App Bundle ready at: apps/android/app/build/outputs/bundle/release/app-release.aab"

build-android-apk:
	cd apps/android && ./gradlew assembleRelease
	@echo "Release APK ready at: apps/android/app/build/outputs/apk/release/app-release.apk"

build-rpm:
	bash packaging/linux/rpm/build-rpm.sh

build-deb:
	bash packaging/linux/deb/build-deb.sh

publish-linux-repo:
	bash scripts/linux-repo/publish-repo.sh --channel "$${CHANNEL:-stable}" \
		--rpm-dir "apps/desktop/src-tauri/target/rpm" \
		--deb-dir "apps/desktop/src-tauri/target/deb"

smoke-rpm-almalinux9:
	bash packaging/linux/rpm/smoke-test-almalinux9.sh

smoke-rpm-almalinux10:
	bash packaging/linux/rpm/smoke-test-almalinux10.sh

smoke-deb-ubuntu2204:
	bash packaging/linux/deb/smoke-test-ubuntu2204.sh

smoke-deb-ubuntu2404:
	bash packaging/linux/deb/smoke-test-ubuntu2404.sh

smoke-deb-debian11:
	bash packaging/linux/deb/smoke-test-debian11.sh

smoke-deb-debian12:
	bash packaging/linux/deb/smoke-test-debian12.sh

smoke-channel-switch:
	bash packaging/linux/repo/smoke-test-channel-switch.sh

typecheck:
	bun run --filter shaprint-desktop typecheck

lint:
	cd apps/desktop/src-tauri && cargo fmt --all -- --check
	cd apps/desktop/src-tauri && cargo clippy --all-targets -- -D warnings

check: typecheck lint test
