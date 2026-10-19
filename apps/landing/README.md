# ShaPrint landing page

English Product Hunt landing page, setup guide, and Android-app/website privacy policy for
`https://sha-print.pages.dev`. Built as static HTML/CSS with native SVG hero
motion, TypeScript parallax, and a small click reporter. The desktop and Android
applications are not imported or changed.

## Develop and verify

From the repository root:

```bash
bun install --frozen-lockfile
bun run landing:dev
```

The local Cloudflare Pages preview is at `http://localhost:8788`. This builds the
static site, creates the local counter table, and starts Pages Functions with a
local D1 binding. It does not use production data or require Cloudflare login.

```bash
bun run typecheck
bun run test
bun run landing:build
```

The root checks include both the desktop frontend and landing package. Landing
checks alone are `bun run --cwd apps/landing typecheck` and
`bun run --cwd apps/landing test`. Tests cover Stable/Android/Nightly selection,
asset URL validation, counter aggregation using the real SQL schema, body limits,
origin restrictions, privacy signals, and storage failure.

## Release links

`src/release-catalog.json` is a frozen, verified set of release assets. The page
shows its exact Stable version rather than promising a moving "latest" download.
This makes builds reproducible without network access.

Update and publish manually from repository root whenever a new Stable desktop or
Android release has finished uploading:

```bash
make landing-deploy
```

This command runs these stages sequentially, including with `make -j`:

1. Refresh the newest semantic Stable desktop and Android versions from GitHub.
2. Require all seven exact release assets and verify their public download URLs.
3. Run the landing TypeScript check and tests.
4. Build and upload to Cloudflare Pages project `sha-print`, production branch `main`.

It uses the local Wrangler login. No GitHub Actions deployment, GitHub secrets,
CI token setup, automatic release trigger, commit, or push is involved. It deploys
this local checkout, including current uncommitted changes; `--commit-dirty=true`
makes that deployment metadata explicit. Review local changes before publishing.
An unsuccessful refresh, link check, test, or build stops the command before the
upload. Run it once the complete release is available, not while packages are
still uploading. Nightly, draft, and prerelease versions are excluded.

Other local commands:

```bash
make landing-dev      # Local preview at http://localhost:8788
make landing-refresh  # Update catalog and verify downloads, without publishing
make landing-check    # Typecheck and landing tests
make landing-build    # Build using current catalog, without network refresh/upload
```

`refresh:releases` reads public GitHub metadata and accepts an optional
`GITHUB_TOKEN` for authenticated API limits. No token is required for ordinary
public lookups. Missing assets, API errors, invalid responses, or more than 1,000
release-history entries fail before replacing the catalog. Download HEAD requests
never receive the GitHub token. Screenshots and product copy remain source updates;
a new release does not invent or recapture screenshots.

## Cloudflare Pages deployment

If Wrangler is already logged in, use `make landing-deploy` directly. Otherwise,
log in once and create the project if it does not exist:

```bash
cd apps/landing
bunx --no-install wrangler login
bunx --no-install wrangler pages project create sha-print --production-branch main
```

The target site is `https://sha-print.pages.dev`. Project availability and account
bindings must be established in the intended Cloudflare account. Production is
always selected explicitly with `--branch main`, even from a local feature branch.
There is no automatic publication on an app release, push, or merge. Disable any
separate automatic Cloudflare Git production build if one was configured, so that
publishing remains manual.

Production bindings are owned by the Pages project settings. There is no checked-in
production Wrangler configuration with a dummy D1 ID; `wrangler.local.jsonc` is only
used for local development. Initialize D1 and add the `DOWNLOAD_METRICS` production
binding as described below before collecting download clicks.

Wrangler runs from `apps/landing`, with `functions` alongside `dist`, so it includes
the counter Function. Dragging only `dist` into the dashboard does not deploy that
Function. The build copies real Android screenshots from `img/android` at repository
root. Never put credentials in source files, command arguments, or commits.

## Measurement setup

### Visits

In the Pages project, enable Cloudflare Web Analytics. Its supported Pages
integration injects the beacon. No analytics token is embedded in this source.
The site's CSP permits that beacon and its collection endpoint.

Cloudflare Web Analytics does not support custom click events. Download intent
uses the separate counter below; click counts are not unique users, successful
downloads, or installations.

### Download clicks

Create the production D1 database and initialize it:

```bash
cd apps/landing
bunx wrangler d1 create shaprint-landing-metrics
bunx wrangler d1 execute shaprint-landing-metrics --remote --file migrations/0001_download_counts.sql
```

In Pages > sha-print > Settings > Bindings, add a production D1 binding named
`DOWNLOAD_METRICS` pointing to that database, then redeploy. For preview
measurement, use a separate preview database and initialize its schema rather
than pointing previews at production.

The endpoint returns 503 when the binding or schema is missing. Package links
still point directly to GitHub and remain usable if metrics are unavailable,
JavaScript is disabled, or reporting is blocked. The database contains only UTC
date, allowlisted package ID, and count. No visitor identifier, IP, referrer,
Network Channel value, or print job content is saved by this counter. Cloudflare
still handles normal HTTP requests at the infrastructure layer. The client and
endpoint honor DNT and GPC for click reporting.

Read production aggregate counts using the authenticated dashboard or CLI:

```bash
bunx wrangler d1 execute shaprint-landing-metrics --remote --command 'SELECT day, package, clicks FROM download_counts ORDER BY day DESC, package;'
```

The endpoint is not an anti-fraud system. Automated requests and repeat clicks
can affect totals; use these figures as directional launch measurements.

Check local measurement after clicking a package in the preview:

```bash
cd apps/landing
bunx wrangler d1 execute shaprint-landing-metrics-local --local --config wrangler.local.jsonc --command 'SELECT day, package, clicks FROM download_counts;'
```

Before announcing the site, verify the deployed home page and guide, all package
URLs, the analytics beacon, and a download click reaching the production counter.
The local implementation does not establish that these account bindings are live.

## Assets and content

- `public/assets/brand.svg` copies the existing desktop favicon's approved mark.
- Android screenshots come only from `img/android/`: raw ADB captures from the
  running Kotlin/Compose Android app on an Android 16 emulator. The build copies
  them unchanged into `/assets/`. The hero uses the real Discovery screen; the
  Android section uses a single large screenshot with Discovery, Printers, and
  Servers choices instead of stacked thumbnails. Native radio inputs and CSS
  switch the visible screenshot, including without JavaScript; arrow keys work
  through native browser behavior. Each view links to its full PNG, and the
  section CTA targets the Android download. See `img/android/README.md` for capture provenance and limitations.
  Generated Android mockups and their generator code have been removed.
- Instrument Sans is self-hosted; its OFL is next to the font.
- The network diagram and social preview are repository-native SVG graphics.
- The hero uses a separate compact diagram on screens up to 800 px wide. It
  names the computer client, Android client, server, and USB printer and states that the server stays on.
- At build time, both diagram SVGs are inlined so their layers can move without
  an animation library. A synchronized nine-second scene sends visible documents
  independently from a laptop and an Android phone directly to the server,
  animates processing, and feeds a full sheet out of the
  printer. Phase captions and a progress rail follow the same clock. This is an
  illustration, not live print-job status.
- Scroll and fine-pointer input move diagram layers at different depths. The
  frame loop settles when pointer input stops, and motion pauses when the diagram
  is offscreen or the document is hidden. Touch uses smaller scroll displacement.
- Pause motion stops the sequence and resets parallax. Reduced motion disables
  both, including when the system preference changes while the page is open.
  Without JavaScript or IntersectionObserver, the diagram remains static and all
  package links continue to work. No motion preference is stored.
- Download rows label the target distribution and distinguish system packages
  from the AppImage desktop interface. No OS detection hides other packages.
- Open Graph and Twitter metadata include an explicit 1200 × 630 preview image
  and image descriptions. Instrument Sans is preloaded from the local asset.
- `desktop-preview.png` is captured from the existing desktop React UI at
  1200×800 with a simulated, non-sensitive configuration. Its caption explicitly
  identifies the configuration as an example. It is not evidence of physical
  printing or operating-system qualification.
- No testimonials, install counts, universal printer compatibility, perpetual
  pricing promise, or whole-product license claim are added.

The [Product Hunt copy draft](../../docs/launch/product-hunt.md) includes a
tagline, short description, and first comment for the maker to review. It has not
been submitted or posted. Real user trials on the public site remain a launch
task after Cloudflare publication.

## Compatibility

No breaking changes. A Stable client can still print to a Nightly server and a
Nightly client to a Stable server on the same basis as before. The dedicated
printing ports, wire protocol, Network Channel authentication, queue URI paths,
and TLS identity storage are untouched. Settings, Trusted Servers, shared printer
selections, and Network Channel storage remain unchanged and downgrade-compatible.
`SETTINGS_RISK=true` is not required for this website change.

## References

- [Cloudflare Pages build configuration](https://developers.cloudflare.com/pages/configuration/build-configuration/)
- [D1 bindings in Pages Functions](https://developers.cloudflare.com/pages/functions/bindings/#d1-databases)
- [Cloudflare Web Analytics for Pages](https://developers.cloudflare.com/pages/how-to/web-analytics/)
- [Cloudflare Web Analytics custom-event limitation](https://developers.cloudflare.com/web-analytics/faq/#does-web-analytics-support-custom-events)
- [Reduced-motion preferences](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/At-rules/@media/prefers-reduced-motion)
- [Intersection Observer](https://developer.mozilla.org/en-US/docs/Web/API/Intersection_Observer_API)


## Play Store privacy policy

`/privacy/` covers the Android client (`io.shaprint.client`) and the website. It
names ShaPrint, includes the approved privacy contact `ardlifirman17@gmail.com`,
and describes document transfers, local caches, saved server information, Network
Channel handling, Android backup, permissions, deletion, and website providers.
The source review and remaining listing checks are in
`docs/launch/privacy-policy-review.md`.

Use `https://sha-print.pages.dev/privacy/` in Play Console after publishing via
`make landing-deploy` and confirming the live page is accessible without login.
The website policy does not add the separately required in-app privacy link or
complete the Play Console Data safety form. Update policy wording alongside any
future change to SDKs, backup, retention, permissions, or data destinations.

## SEO and sharing metadata

`src/seo.ts` owns the production origin and metadata for the home page, setup
guide, and privacy policy. The build renders unique titles/descriptions, canonical
URLs, Open Graph and Twitter cards, and inline JSON-LD into the served HTML. Search
and sharing metadata does not depend on browser JavaScript. Serve `dist`, not the
unbuilt `public` templates.

The home graph describes the WebSite and the desktop/Android applications. App
versions and download URLs come from the same release catalog as the visible
package links, so `make landing-deploy` refreshes both together. Guide/privacy
breadcrumbs match visible navigation. No price, offer, review, rating, search
endpoint, or organization is invented. This factual graph does not assert Google
software-app rich-result eligibility; the product has no published price/review
data to support those fields.

The same page registry generates `dist/sitemap.xml` and `dist/robots.txt`. It lists
only the three public canonical content URLs; the API is excluded from crawling,
and the missing-page response remains HTTP 404 with `noindex`. No artificial
`lastmod` timestamps or unsupported language variants are added. Robots directives
are crawl hints, not access controls.

JSON-LD is escaped to prevent a closing-script string from creating executable
markup. Build-time SHA-256 hashes are inserted into `dist/_headers` for the exact
JSON-LD bodies. The source `_headers` placeholder must be processed by the build;
`unsafe-inline` is not enabled. Local browser checks verify the served hashes and
metadata without JavaScript, plus correct 404 behavior.

After manual production deployment, verify the public URLs and submit
`https://sha-print.pages.dev/sitemap.xml` in the site's Google Search Console
property. Account verification, Google crawling/indexing, ranking, and rich-result
display have not been performed or guaranteed by this implementation.

References: [Google site-name guidance](https://developers.google.com/search/docs/appearance/site-names),
[software-app structured data](https://developers.google.com/search/docs/appearance/structured-data/software-app),
[breadcrumb structured data](https://developers.google.com/search/docs/appearance/structured-data/breadcrumb).
