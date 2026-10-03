.PHONY: help keys release-patch release-minor release-major test typecheck check lint

help:
	@echo "Available commands in ShaPrint:"
	@echo "  make keys            - Generate Minisign signing key pair for Tauri updater"
	@echo "  make release-patch   - Bump patch version (x.y.Z+1), commit, and create git tag"
	@echo "  make release-minor   - Bump minor version (x.Y+1.0), commit, and create git tag"
	@echo "  make release-major   - Bump major version (X+1.0.0), commit, and create git tag"
	@echo "  make test            - Run all frontend and backend tests"
	@echo "  make typecheck       - Run frontend TypeScript typecheck"
	@echo "  make lint            - Run cargo fmt check and cargo clippy"

keys:
	bun run scripts/generate-keys.ts

release-patch:
	bun run scripts/release.ts patch

release-minor:
	bun run scripts/release.ts minor

release-major:
	bun run scripts/release.ts major

test:
	bun run --filter shaprint-desktop test
	cd apps/desktop/src-tauri && cargo test

typecheck:
	bun run --filter shaprint-desktop typecheck

lint:
	cd apps/desktop/src-tauri && cargo fmt --all -- --check
	cd apps/desktop/src-tauri && cargo clippy --all-targets -- -D warnings

check: typecheck lint test
