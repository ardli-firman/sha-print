# Manual landing deployment with local Wrangler

---
status: accepted
---

Publish the landing page explicitly with `make landing-deploy` from the maintainer's
local checkout and existing Wrangler login. The user chose manual publication over
release-triggered GitHub Actions. Remove the proposed landing workflows and GitHub
secret setup wizard; app release workflows remain independent of website publishing.
No new GitHub deployment secrets or unattended credentials are required.

The Make target refreshes Stable desktop and Android release links, validates all
seven package assets and their public availability, runs landing checks, then builds
and uploads to the `sha-print` Cloudflare Pages project with production branch
`main` explicitly selected. Each stage runs sequentially, including under parallel
Make. A failed pre-upload stage prevents publishing. The current local checkout is
the source, including reviewed uncommitted changes, and Wrangler marks it dirty in
deployment metadata. Local build and refresh targets remain usable without uploading.

This trades automatic synchronization for deliberate control of when updated links
and source changes go public. After each completed Stable desktop or Android release,
a maintainer runs the same Make target. Screenshots remain real app captures updated
in source. A separate Cloudflare Git production build must not publish automatically
if manual publication is required. Project creation and the D1 binding remain
one-time account setup.

No breaking changes. Stable clients can still print to Nightly servers and Nightly
clients to Stable servers under the same supported-version assumptions. Ports,
IPPS, Network Channel authentication, queue URIs, and TLS identity storage are
unchanged. Settings, Trusted Servers, shared printer selections, and Network Channel
storage remain backward-compatible on downgrade; `SETTINGS_RISK=true` is not required.
