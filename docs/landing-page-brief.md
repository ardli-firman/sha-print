# ShaPrint landing page brief

Status: discovery confirmed by the user; the second landing-page iteration is in
`apps/landing`. Cloudflare publication and account-side analytics setup remain
pending because the local CLI is not authenticated.

## Agreed decisions

- Launch channel: Product Hunt.
- Primary audience: people managing IT for small teams.
- Page language: English.
- Primary action: download the Stable release.
- Secondary action: read the setup guide.
- Business model: undecided. Do not promise that ShaPrint will be free forever.
- Lead with sharing an existing printer with a small team. Include older office
  printers without Wi-Fi as a concrete use case.
- Address frustration with Windows printer sharing as a user problem, without
  promising to resolve every printer, driver, or network error.
- Emphasize Windows and Linux downloads; present Android as an additional option
  with its role explained.
- Hero visual: a diagram connecting a USB printer to a server computer and then
  to multiple client computers. Follow it with an actual app screenshot.
- Use the existing teal and white brand identity and approved logo geometry.
- Hosting target: Cloudflare Pages.
- Requested public address: `https://sha-print.pages.dev`. The user chose this
  address; Cloudflare project-name availability has not been checked.
- Launch measurement: count visits and Download clicks by platform. A click is
  evidence of download intent, not a completed download or installation.
- Initial page order: hero with diagram and Download CTA; office-printer
  problems; how server/client setup works; app screenshot; Windows/Linux download
  choices with Android separately; FAQ, full guide, and help links.
- Omit pricing while the business model is undecided.
- Download interaction: show Windows and Linux package choices on the page,
  linking to Stable release packages. Keep all choices visible so an IT manager
  can download for another computer. Present Android separately.
- Explain the server/client setup briefly in a "How it works" section and link
  to a complete setup guide. Explain that the server stays on for printing.
- Include FAQ coverage for printer compatibility, Windows printer sharing,
  and cross-VLAN use.

## Product language

Use the definitions in [CONTEXT.md](../CONTEXT.md), including server, client,
shared printer, print job, and Network Channel. No new domain terms have been
agreed during this interview.

## Evidence and limits

Release availability checked on 2026-10-17:

- [Desktop Stable v3.4.4](https://github.com/ardli-firman/sha-print/releases/tag/v3.4.4)
  has Windows x64 installers and Linux amd64 packages.
- [Android v1.0.3](https://github.com/ardli-firman/sha-print/releases/tag/android-v1.0.3)
  has an APK.
- Package availability does not establish that every advertised platform or
  printer configuration has passed runtime qualification.
- The Rust crate declares MIT in its Cargo metadata, but whole-product license
  coverage needs clarification before making a prominent open-source claim.

## Agreed copy direction

Approved during discovery; final page copy will still need review in context:

> Your office printer. Shared with the whole team.

> Share your existing USB printer through a computer running ShaPrint. No
> built-in printer Wi-Fi required.

The setup explanation must distinguish the printer's lack of built-in Wi-Fi
from the server computer's need for network connectivity.

## Setup and claim constraints

- The printer needs a working local driver on the server. USB printing is
  documented, including Epson L3210 in the desktop smoke-check instructions;
  this is not evidence of universal printer compatibility.
- For printing, the server must be on, connected, and running ShaPrint with
  sharing enabled. Do not imply that the printer itself gains independent
  network connectivity.
- LAN and cross-VLAN printing do not require a third-party cloud printing
  service. Cross-VLAN use still requires network reachability; discovery may
  require multicast routing or reflection, with manual address entry as fallback.
- Use the visual rules in the brand guidelines, but do not copy their stale
  port number or absolute security claims into landing copy.

Sources: [desktop README](../apps/desktop/README.md),
[ADR 0010](adr/0010-render-client-raster-through-server-driver.md),
[ADR 0014](adr/0014-unique-ports-and-cross-vlan-reachability.md),
[brand guidelines](branding/brand-guidelines.md).

## Confirmed delivery scope

- Implement a separate static landing-page app in this repository, preserving
  the existing desktop app's behavior.
- Provide an English setup-guide page linked from the landing page.
- Verify responsive layouts, keyboard navigation, actual download destinations,
  and the measurement behavior before handoff.
- Prepare Cloudflare Pages deployment instructions/configuration. Publication
  depends on available Cloudflare access and the requested project name.
- Review the complete page copy against the product evidence above.

## Implementation verified locally

- Download click aggregation runs through a Pages Function and D1. Local browser
  clicks for all seven packages have been exercised; production binding is still
  pending. Cloudflare Web Analytics does not support custom events per its
  [FAQ](https://developers.cloudflare.com/web-analytics/faq/) checked on
  2026-10-17, so Download clicks require a separate measurement mechanism.
- The frozen release catalog resolves desktop Stable and Android separately,
  excluding draft and prerelease releases. Package URLs have been checked.
- The existing logo and a captured desktop UI screenshot are used. The screenshot
  is clearly captioned as an example configuration.

## Second iteration

The user approved a second iteration focused on mobile clarity, package choice,
and launch sharing:

- A compact mobile diagram appears between the introduction and primary CTA,
  with the server availability requirement in its caption.
- All seven download choices remain visible. Linux package rows explain distro
  targets and the difference between system packages and the AppImage interface.
- Open Graph and Twitter metadata describe the preview image. The image includes
  the server computer, rather than implying the printer connects independently.
- Draft Product Hunt listing copy and a maker comment are recorded separately in
  [the launch copy file](launch/product-hunt.md). Nothing has been posted.
- Cloudflare publication, production measurement, and real user trials on the
  live site remain pending. The local CLI is still unauthenticated.

No architectural trade-off requiring an ADR, and no new glossary term, has
been settled during this interview. Keep this product brief separate from the
domain glossary.

## Compatibility

The hero is a full animated scene: visible documents leave a computer client and
an Android client independently, travel directly to the server, and are routed to
the printer before a full sheet is fed out. The phone displays a raw screenshot
from the running Android app. A separate section shows real screenshots of
Discovery, Shared Printers, and Trusted Servers in a single large, selectable
gallery, with full-size PNG links. The Android section uses a dark slate surface,
a short headline, and native screenshot choices rather than stacked thumbnails.
The captures show the actual state without a connected server. Generated UI
mockups are not accepted as screenshots.
Client, server, printer, processing ring, paper, phase captions, and progress rail
are choreographed on a synchronized nine-second loop. Scroll/pointer parallax
remains an additional depth treatment. The headline and download controls remain
stationary. Pause/Resume and system reduced-motion handling are included. The
scene is explicitly an illustration, not live print-job activity.

This brief changes no application behavior. Stable clients can still print to
Nightly servers and Nightly clients to Stable servers on the same basis as before.
Persisted settings, Trusted Servers, shared printer selections, and Network
Channel storage are unchanged, including downgrade compatibility. There is no
breaking change and this work does not require SETTINGS_RISK=true.
