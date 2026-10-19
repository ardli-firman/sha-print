# Landing page verification

Observed locally on 2026-10-17, branch `feat/product-hunt-landing`.

## Automated checks

| Command                                       | Observed result                                                                             |
| --------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `bun install --frozen-lockfile`               | Checked 179 installs across 316 packages; no changes                                        |
| `bun run typecheck && bun run test`           | Desktop and landing typechecks exited 0; 84 desktop tests passed; 14 landing tests passed   |
| `bun run --cwd apps/landing refresh:releases` | Verified desktop Stable 3.4.4 and Android 1.0.3; refreshed seven package links              |
| `bun run landing:build`                       | Static home page, guide, privacy page, browser script, and package links built successfully |
| `git diff --check`                            | No whitespace errors                                                                        |

The landing tests exercise release filtering and exact asset URLs, plus the
counter with the real SQLite schema, origin restrictions, body limits, DNT/GPC,
and absent or failing storage. Rust code was not changed; no Rust-suite result is
claimed.

## Runtime and browser checks

`bun run landing:dev` launched the actual Cloudflare Pages local runtime on port
8788 with a local D1 binding initialized from the migration. Playwright used the
installed Google Chrome to exercise the built site.

- Home page checked at widths 1440, 1024, 768, 390, and 320 px: no horizontal
  overflow, failed images, browser exceptions, or console errors observed.
- Hero Download navigated to the package section; FAQ opened using the keyboard.
- A real browser click on the Windows installer link sent a counter request;
  `/api/download` returned 204. Reading the local D1 table confirmed the
  `windows-exe` aggregate. The navigation was suppressed by the QA runner to
  avoid downloading an installer; the seven package destinations were checked
  separately using real HTTP HEAD requests, all returning GitHub's 302 redirect.
- A separate real HTTP counter request for `linux-deb` returned 204 and appeared
  in the local D1 table.
- Home page and guide/privacy pages checked at desktop and mobile widths.
- axe found no violations on the home page at 1440 and 390 px, or on the privacy
  page. It initially reported duplicate unnamed complementary landmarks in the
  guide. Both asides were named; the guide's subsequent axe scan returned no
  violations. Automated scanning does not establish complete accessibility.
- With JavaScript disabled, all seven package links remained present and FAQ
  disclosure remained functional.
- With Global Privacy Control enabled, clicking Download emitted no counter
  request.
- An unknown path displayed the custom 404 page.

The desktop screenshot uses the real existing React interface with a simulated
example configuration, identified as such in its caption. It is not a live
printer test or new evidence of printer-model compatibility.

## Publication status

`bunx wrangler whoami` reported that the CLI is not authenticated. No Cloudflare
project was created or published, and availability of `sha-print.pages.dev` is
unverified. Production D1 binding, Cloudflare Web Analytics, and a deployed-site
click still need verification after account setup. Deployment instructions are
in [the landing README](../apps/landing/README.md).

## Compatibility

No breaking changes. Stable clients can still print to Nightly servers and
Nightly clients to Stable servers on the same basis as before. Printing ports,
IPPS protocol, Network Channel authentication, client queue URI paths, and TLS
identity/fingerprint storage are unchanged. Settings, Trusted Servers, shared
printer selections, and Network Channel storage retain their existing downgrade
compatibility. `SETTINGS_RISK=true` is not required for this change.

## Second iteration verification

Observed on 2026-10-17 after the mobile, download-choice, and share-preview changes:

- `bun run typecheck && bun run test` again exited successfully: both frontend
  typechecks, 84 desktop tests, and 14 landing tests passed.
- `bun run landing:build` produced the updated static pages and assets.
- Browser checks at widths 1440, 1024, 800, 768, 520, 390, 375, and 320 px observed
  no horizontal overflow, broken images, page exceptions, or console errors.
- The hero selected `network-mobile.svg` at mobile widths and `network.svg` on
  desktop. At 390 × 844 the compact diagram occupied y=384–568 and the main
  Download button y=624–678. At 320 × 844 the main Download button occupied
  y=587–641, so the diagram, server requirement, and CTA were in the first screen.
- axe found no violations on the home page at 1440 and 390 px, nor on the guide
  or privacy pages. These are automated checks, not a complete accessibility audit.
- The FAQ worked from the keyboard. With JavaScript disabled, package links and
  FAQ remained usable. GPC still suppressed click reporting.
- All seven rendered package URLs exactly matched the release catalog and
  returned 302 to real HTTP HEAD requests. Clicking each link in a real browser
  returned 204 from the local counter. Reading local D1 confirmed all seven
  package identifiers were present.
- All 16 distinct internal page, fragment, and screenshot targets resolved.
- The served share image was a 1200 × 630 PNG. Open Graph and Twitter title,
  description, image, and image-description fields were present; image metadata
  matched the actual PNG dimensions.
- Product Hunt draft fields were checked: tagline 41 characters, description
  242 characters. Both fit the smaller limits documented in the linked Product
  Hunt help-center guide. The draft has not been submitted or posted.
- `bunx wrangler whoami` still reported no authenticated account. No public
  deployment, production analytics verification, or live-site user trial is
  claimed.

The second iteration changes the landing page, its assets, and launch documents
only. The compatibility statement above continues to apply.

## Current full-scene hero verification

The hero uses a nine-second animated print journey. This replaces the earlier
small pulse treatment. In the actual local Pages runtime, Playwright and Google
Chrome exercised:

- Autoplay without pointer input. The document's animation clock advanced.
- Poses for send, route, and print. The visible document traveled approximately
  212 SVG units between sampled poses; the printed sheet traveled approximately
  157 SVG units and was revealed through the printer's output clip. These are
  substantial object animations, not only connection-line pulses.
- The phase captions changed from sending to routing to printing. Server
  processing and the printer's mechanical response share the same scene clock.
- Pause/Resume froze and restarted the actual animation timeline. Scrolling the
  scene offscreen paused it. Scroll parallax still produced distinct layer offsets.
- Changing reduced-motion preference canceled the scene animations, restoring
  static artwork and hiding the control.
- At mobile width, the compact scene played and the hidden desktop scene stayed
  paused. A JavaScript-disabled context retained static artwork and all seven
  download links.
- Eight widths from 320 to 1440 px showed no horizontal overflow. axe found no
  violations at desktop and phone sizes. No console/CSP errors or page exceptions
  were observed. A real package click still returned 204 from the local counter.
- `bun run typecheck && bun run test` passed both frontend typechecks and all
  98 existing tests. `bun run landing:build` successfully bundled the scene and
  controller and inlined the new artwork.

A fresh desktop and mobile browser recording demonstrates a full autoplay cycle.
The scene is explicitly illustrative; it is not evidence of a real print job.
Application printing behavior and persisted state are unchanged. The compatibility
statement above remains applicable. Cloudflare publication still requires account
authentication.


## Earlier Android client iteration (superseded)

This earlier iteration included a computer client and an Android phone in both layouts.
Each has a separate animated document path directly to the same server. The phone
has its own motion and parallax layer and displays the repository's existing
Android print-preview artwork. A dedicated Android section shows the interface at
a larger size and links to a full-size preview. This is preview artwork, not a new
Android runtime capture.

Local Pages runtime checks in Chrome verified the phone document is visible at
sampled send poses and moves more than 70 SVG units toward the server in each
layout. The screen asset loads, the larger preview decodes, and its full-size link
opens successfully. Both computer and phone remain present at desktop and narrow
widths. New browser recordings capture the actual autoplay loop.

The full hero browser checks passed again: autoplay, pause/resume, scroll parallax,
offscreen pause, reduced-motion cancellation, touch layout, JavaScript-disabled
fallback, all seven package links, and the local counter response of 204. All eight
widths from 320 to 1440 px had no horizontal overflow; axe found zero violations at
1440 and 390 px; no page or console errors were observed.

`bun run typecheck && bun run test && bun run landing:build` passed both typechecks,
84 desktop tests, 14 landing tests, and the landing build. Application code and
protocols are unchanged. No breaking changes: Stable clients can still print to
Nightly servers and Nightly clients to Stable servers on the same basis as before.
Persisted settings, Trusted Servers, shared printer selections, and Network Channel
storage remain backward-compatible when downgrading from Nightly to Stable.


## Real Android screenshots replace generated artwork

The four generated `img/screenshot-[1-4]-*.png` images and landing copies/wrappers
were deleted. Their screenshot generator templates were removed; running
`bun scripts/generate-playstore-assets.ts` now only creates the icon and feature
branding graphic, and did not recreate the deleted screenshots.

`./gradlew :app:assembleDebug --console=plain` built successfully. The APK was
installed on the Android 16 emulator via ADB, cold-launched with status `ok`, and
exercised through the real Shared Printers, Discovery, and Trusted Servers tabs.
Starting a scan showed the real scanning UI. Raw screenshots were captured with
`adb exec-out screencap -p`; source/build metadata and SHA-256 checksums are in
`img/android/README.md`. The unit-test task
`./gradlew :app:testDebugUnitTest --console=plain` succeeded with cached tasks.
The initial offline build could not resolve missing dependencies; the subsequent
online build succeeded. The app is native Kotlin/Compose, not Flutter.

These captures show the real app with no connected or trusted server. No successful
print submission or discovery is claimed. The hero uses the captured Discovery
screen without a synthetic button over it. The larger Android section links to
all three full-resolution PNGs. The landing build copies source PNGs unchanged.

No breaking changes. Stable client ↔ Nightly server printing compatibility is
unchanged in both directions. Settings, Trusted Servers, shared printer selections,
and Network Channel storage remain downgrade-compatible; application protocols
and persisted state were not modified.

Landing verification after replacement passed `bun run typecheck && bun run test &&
bun run landing:build` (84 desktop tests and 14 landing tests). Chrome verified the
scene lifecycle and phone document movement, eight widths from 320 to 1440 px,
zero axe violations at 1440 and 390 px, and no console/page errors. All three
served PNGs matched their raw ADB source bytes by SHA-256; the removed generated
PNG and SVG endpoints returned 404. All gallery images decoded, and the Android
full-size screenshot link opened its raw PNG. A new recording shows the actual
capture inside the animated hero phone.


## Android section layout revision

The previous tall screenshot stack was replaced with a dark slate section, a
shorter headline, one large screenshot, three native radio choices, and a CTA
pointing directly to the Android download. Each selection has its own full-size
PNG link. The original ADB captures are unchanged. Selecting a screen is manual;
there is no automatic carousel or transition motion.

In the local Pages runtime, Chrome exercised all three choices at eight widths
from 320 to 1440 px. Exactly one screenshot was visible and no horizontal overflow
occurred. Keyboard arrows changed selection and wrapped across the radio group.
All choices also worked with JavaScript disabled. All three full-size links opened
the correct raw PNG, whose served bytes matched the original captures by SHA-256.
The CTA reached `#android-download`, which contains the Android APK link. axe found
zero section violations for every screen at 1440 and 390 px; no page exceptions
were observed. Desktop and phone screenshots were visually inspected. The desktop
section measured approximately 821 px high at a 1440 px viewport.

`bun run typecheck && bun run test && bun run landing:build` passed both typechecks,
84 desktop tests, 14 landing tests, and the landing build. No app behavior or state
changed. No breaking changes: Stable client ↔ Nightly server printing remains
compatible in both directions, and persisted settings, Trusted Servers, shared
printer selections, and Network Channel storage remain downgrade-compatible.

## Earlier automatic release proposal (superseded)

Added `.github/workflows/landing-deploy.yml`. It follows successful Release Desktop
and Release Android workflows, manual published/edited releases, landing pushes to
`main`, and manual dispatch. It checks out trusted `main`, refreshes a transient
catalog, validates seven public download links, runs typecheck/tests/build, and
uploads production with Wrangler. Failed release workflows are excluded; failed
validation prevents the upload. Nightly and draft/prerelease catalogs are excluded
by the release selector. Production uploads share a serialized concurrency group.

Observed verification:

- `bun run --cwd apps/landing refresh:releases`: verified desktop Stable 3.4.4 and
  Android 1.0.3 against the real GitHub Releases API.
- `bun run --cwd apps/landing verify:downloads`: all seven public asset HEAD
  requests followed redirects successfully and returned HTTP 200.
- `bun run typecheck && bun run test && bun run landing:build`: both typechecks,
  84 desktop tests, 19 landing tests, and build passed. The five new refresh tests
  cover pagination beyond the first page, a later API failure, an invalid API
  response, a newest incomplete release, and the bounded-history failure.
- `actionlint` 1.7.7 checked the landing deployment and CI workflows with no errors.
- `bash -n scripts/setup-landing-cloudflare.sh` passed. The human credential wizard
  was inspected without executing its interactive secret writes. Its helpers are
  copied unchanged from the wizard template. It writes only the two Cloudflare
  secret names referenced by the deployment workflow, never a local token file.
- `bunx --no-install wrangler --version` from `apps/landing` resolved the installed
  Wrangler 4.151.1 used by the workflow.

The repository's Actions secret-name listing contained no Cloudflare credentials
at verification time. The new workflow remains local and is not yet merged into
`main`; no live Actions run, production upload, Cloudflare project creation, or D1
binding is claimed. Activation requires the one-time Cloudflare project/D1 setup,
the two GitHub secrets, and merging the workflow into `main`. The wizard and
`apps/landing/README.md` document that setup. Avoid a competing Cloudflare Git build
that would publish the checked-in frozen catalog over the refreshed one.

No breaking changes. Stable client ↔ Nightly server printing remains compatible in
both directions, and persisted settings, Trusted Servers, shared printer selections,
and Network Channel storage remain backward-compatible on downgrade. No application
protocol or settings migration was changed.

## Current publication path: manual Makefile

At the user's request, removed both proposed landing Actions workflows and the
GitHub secret wizard. Publication is manual with `make landing-deploy`, using the
local Wrangler login. The existing desktop/Android release workflows are unchanged.
The accepted decision is now `docs/adr/0021-manual-landing-deployment.md`.

Observed verification:

- Local `wrangler whoami` confirmed an authenticated OAuth session with Pages write
  access. No credentials were copied into source or GitHub secrets.
- `make landing-refresh && make landing-check && make landing-build` succeeded.
  Refresh selected desktop Stable 3.4.4 and Android 1.0.3, all seven package HEAD
  requests returned 200, the landing typecheck and 19 tests passed, and build passed.
- `bun run typecheck && bun run test` passed both workspace typechecks, 84 desktop
  tests, and 19 landing tests (103 total).
- `make -n landing-deploy` showed refresh, URL verification, typecheck, tests, and
  build/upload in order. The package deploy command explicitly selects Pages
  project `sha-print` and production branch `main`.
- An isolated temporary command stub exercised `make -j8 landing-deploy`: success
  preserved stage order; failures at refresh, URL verification, typecheck, or tests
  stopped before the upload command. Upload was stubbed in this check.
- `git diff --check` passed. No real production upload or project/D1 creation was
  performed during verification; the publish target is ready for the user to run.

No breaking changes. Stable clients can still print to Nightly servers and Nightly
clients to Stable servers under the same supported-version assumptions. Persisted
settings, Trusted Servers, shared printer selections, and Network Channel storage
remain backward-compatible on downgrade. Protocol, ports, and TLS identity storage
are unchanged.

## Privacy policy for the Android listing

Expanded `/privacy/` from website-only measurement text into a named ShaPrint
Android-app and website policy. It includes the user-approved privacy email,
selected documents and job metadata, server recipients, local storage, Keystore
scope, Android backup, permissions, caches, deletion, provider handling, and a
revision date. The full implementation review and remaining Play-listing checks
are in `docs/launch/privacy-policy-review.md`.

`make landing-build` and `bun run typecheck && bun run test` passed (84 desktop and
19 landing tests). Local Chrome checks verified HTTP 200, metadata, app identity,
contact, anchors, footer navigation, no-JavaScript readability, seven responsive
widths, and zero axe violations at desktop/phone widths. Screenshots were visually
inspected. No real deployment was performed. The public hostname did not resolve
in this session, and an in-app privacy link remains a separate readiness item.

No breaking changes; Stable ↔ Nightly printing and settings/state downgrade
compatibility remain unchanged. Android source, permissions, backup, and data
handling were inspected but not modified.

## SEO enhancement

Added a single SEO registry and build-time rendering for all three public content
pages. Unique titles/descriptions, canonicals, complete sharing metadata, WebSite /
WebPage / software-app graphs, and secondary-page breadcrumb graphs are static in
the generated HTML. Application versions and download targets use the existing
catalog. The build generates sitemap/robots from the same route list and adds CSP
hashes for escaped JSON-LD, without `unsafe-inline`. No offer, price, review, or
rating was fabricated, and no Google rich-result eligibility is claimed.

Observed verification:

- `bun run typecheck && bun run test && make landing-build`: both typechecks, 84
  desktop tests, 24 landing tests, and build passed. Five SEO tests cover catalog
  version coupling, closing-script escaping, unsupported commercial claims,
  breadcrumb destinations, and crawl-route consistency.
- Local Chrome/Playwright with JavaScript disabled checked HTTP 200, exactly one
  document title/description and H1, unique metadata per page, canonical/OG URLs,
  matching sharing titles, valid JSON graphs, and all seven schema download links
  matching visible downloads. The three JSON-LD hashes matched the CSP response
  headers. The PNG share image was 1200 × 630.
- Sitemap contained exactly `/`, `/guide/`, and `/privacy/` at the production
  origin. Robots disallowed `/api/`. An unknown page returned HTTP 404 and `noindex`.
- With JavaScript enabled, all three pages had no horizontal overflow at 1440,
  390, or 320 px. axe found zero violations at 390 px on each page. Two overflowing
  guide command blocks were made keyboard-focusable with labels and focus styles.
  No browser page or console errors, including CSP violations, were observed.
- Guide and phone-width privacy screenshots were visually inspected after adding
  visible breadcrumbs and the descriptive guide heading.

This checks local implementation, not a Google Search Console property or a public
crawler. Production publication, sitemap submission, indexing, rankings, and Google
rich-result validation/display remain unverified. Publication stays manual through
Makefile. No breaking changes: Stable ↔ Nightly printing compatibility and persisted
settings/state downgrade compatibility remain unchanged.
