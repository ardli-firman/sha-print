# Manual nightly desktop releases

---
status: accepted
extends: ADR 0012 (desktop auto-updater)
---

ShaPrint publishes Windows nightly builds for testers as public GitHub prereleases. A maintainer starts each build manually and may choose the current `main` commit or an older commit in `main`'s history. The workflow verifies and builds that exact commit, then records its SHA in the prerelease. It publishes nothing if verification or packaging fails. There is no scheduled nightly build.

Nightly replaces the stable installation on a tester's computer. It keeps ShaPrint's application identity, app data, fixed service ports, and existing update signing key. Its updater reads a separate nightly feed, so an installed nightly only receives later nightly builds. Stable installations continue reading the stable release feed. A tester returns to stable by installing a stable release manually. If a nightly changes the saved settings in a way the stable release cannot read, the tester may need to configure ShaPrint again; that risk must appear in the affected nightly's release notes.

Each successful nightly gets a unique SemVer prerelease version that increases with workflow build order, even when its source commit is older. Each version has its own GitHub prerelease and immutable source tag. The nightly feed points to the newest published nightly. After a new release and its signed update metadata are available, the workflow moves the feed to that release. It keeps the ten most recent nightly prereleases and removes older nightly releases and their tags. The stable release workflow must ignore nightly tags.

The separate feed prevents an automatic switch between release channels. Keeping the same application identity avoids competing instances for ShaPrint's fixed ports, while sharing the signing key avoids provisioning another release secret. The source SHA and build-order version are both visible because choosing an older commit does not imply an older installer version.

Implementation is tracked in GitHub issue #87.
