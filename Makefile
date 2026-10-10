.PHONY: help keys release-nightly release-patch release-minor release-major test test-android build-android-debug apk-debug typecheck check lint

help:
	@echo "Available commands in ShaPrint:"
	@echo "  make keys                - Generate Minisign signing key pair for Tauri updater"
	@echo "  make release-nightly     - Trigger manual Nightly release from main (optional: SHA=<sha> SETTINGS_RISK=true)"
	@echo "  make release-patch       - Bump patch version (x.y.Z+1), commit, and create git tag"
	@echo "  make release-minor       - Bump minor version (x.Y+1.0), commit, and create git tag"
	@echo "  make release-major       - Bump major version (X+1.0.0), commit, and create git tag"
	@echo "  make test                - Run all frontend, backend, and nightly script tests"
	@echo "  make test-android        - Run Android client unit tests"
	@echo "  make build-android-debug - Build Android client debug APK (alias: make apk-debug)"
	@echo "  make typecheck           - Run frontend TypeScript typecheck"
	@echo "  make lint                - Run cargo fmt check and cargo clippy"

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

typecheck:
	bun run --filter shaprint-desktop typecheck

lint:
	cd apps/desktop/src-tauri && cargo fmt --all -- --check
	cd apps/desktop/src-tauri && cargo clippy --all-targets -- -D warnings

check: typecheck lint test
