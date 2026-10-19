# Product Hunt launch copy

Draft for the maker to review. Nothing has been submitted or posted.
The landing-page copy and product claims follow the agreed brief.

## Listing fields

Name:

> ShaPrint

Tagline:

> Share the office printer you already have

Description:

> Share an existing USB printer with your team over the local network. ShaPrint runs on Windows and Linux, with an Android client. No printer Wi-Fi required. Keep a working driver on the server and leave that computer on while your team prints.

Website:

> https://sha-print.pages.dev/

Do not submit the URL until the project is published and verified. The requested
Pages project name has not been checked in the user's account.

The tagline is below 60 characters and the description below 260. Product Hunt's
[help-center launch guide](https://help.producthunt.com/en/articles/17350772-launch-guide)
states those limits. Its separate
[preparation page](https://www.producthunt.com/launch/preparing-for-launch)
currently gives a larger description limit; this draft fits the smaller one.

## First comment draft

Hi Product Hunt! ShaPrint helps small teams share an office printer they already
have, including a USB printer without built-in Wi-Fi.

The printer stays connected to a server computer with its working driver. Other
computers connect as clients through ShaPrint over the local network. The server
needs to stay on while the team prints. Windows and Linux packages are available,
plus an Android client.

ShaPrint uses an IPPS print path rather than Windows SMB printer sharing. Print
jobs do not need a third-party cloud printing service. Cross-VLAN setups still
need the right routing and firewall access.

Feedback on the first setup is especially useful: was it clear which computer is
the server, which package to install, and how to connect a client? If you try it,
share your operating systems and printer model. Please keep Network Channel
values and document content private.

## Link preview and assets

- Link-preview image: `apps/landing/public/assets/social.png`, 1200 × 630.
- Editable source: `apps/landing/public/assets/social.svg`.
- The diagram explicitly shows the USB printer, server, and client computers.
- The desktop screenshot is the real React interface with a simulated example
  configuration; do not present it as a physical-printing qualification result.
- The 1200 × 630 image is an Open Graph preview, not a claim that it satisfies
  Product Hunt's separate gallery requirements. Prepare and inspect gallery
  assets against the current submission form before uploading them.

## Before submitting

- Publish to the chosen Cloudflare Pages account and verify the public URL.
- Enable Web Analytics and bind the production download-counter database.
- Check one real package click reaches production measurement and the correct
  GitHub asset. A click does not establish installation success.
- Have a few people try the page, choose a package, and follow the setup guide.
  Record where they hesitate before revising the copy again.
- Review the first comment in the maker's own voice and decide launch timing.
- Keep pricing and whole-product licensing claims out until those decisions are
  settled. Do not promise that every printer is compatible.

## Compatibility

No breaking changes from this launch material or landing-page iteration.
Stable clients can still print to Nightly servers and Nightly clients to Stable
servers as before. Settings, Trusted Servers, shared printer selections, and
Network Channel storage keep their existing downgrade compatibility.
