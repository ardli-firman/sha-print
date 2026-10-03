# Desktop UI/UX flow redesign

---
status: accepted
---

The desktop UI/UX was redesigned to eliminate procedural ambiguity, automate daemon lifecycle
management based on user intent, and deliver a responsive, accessible layout across all Windows
screen dimensions.

## Context

Prior to this decision, the desktop interface exhibited several usability and architectural friction points:
1. **Daemon Lifecycle Exposition**: A permanent `RuntimeStatusPanel` occupied substantial vertical
   space on every screen, requiring users to manually manage `client-proxy`, `server-sharing`, and
   `server-discovery` with Start/Stop buttons. Failing to manually start `client-proxy` after
   installing a queue resulted in silent print failures.
2. **Disjointed Client Onboarding**: Connecting to a server was split across unlinked panels. A user
   had to review an advertised server, copy its address into a manual input, inspect its certificate,
   manually evaluate a raw 64-character hex SHA-256 fingerprint, click approve, request printers,
   and install the queue.
3. **Disconnected Network Channel**: Network Channel configuration lived exclusively in Settings,
   meaning client users often installed queues without realizing an authorization secret was
   required, discovering the problem only when a print job was rejected.
4. **Fragile and Unresponsive Components**: Several panels used raw unstyled `<button>` and `<input>`
   tags with hardcoded CSS breakpoints that degraded or overflowed at narrow window widths (such as
   Windows Snap Assist).

## Decisions

### 1. Intent-Driven Service Management

Supervised services are managed automatically according to user actions:
- Installing or selecting a client queue ensures `client-proxy` is running.
- Toggling local printer sharing ensures `server-sharing` and `server-discovery` are active.
- Primary workspace headers display a compact **System Status Pill** (Ready / Transition / Problem).
- Detailed daemon lifecycle controls and service error messages are relocated to a **System Diagnostics**
  dialog, accessible via the status pill or the Settings workspace.

### 2. Unified Client Connection Wizard

Client connection is consolidated into a single modal wizard (`AddPrinterDialog`):
- **Step 1 (Discover)**: Lists live nearby servers with an option to enter a manual host/IP.
- **Step 2 (Verify)**: If the server certificate was previously approved (`trusted: true`), verification
  is automatically skipped. If new or changed, the SHA-256 fingerprint is presented with visual chunking
  (`XXXX • YYYY • ZZZZ • AAAA`) alongside clear approval actions.
- **Step 3 (Select Printer)**: Displays printers shared by the server for 1-click installation.
- **Step 4 (Network Channel & Finish)**: Prompts for the Network Channel if unconfigured, installs the
  native Windows queue, ensures `client-proxy` is active, and provides a clear completion notice.

### 3. Client Queue Visibility

The primary Client view ("Printers") displays all installed client queues with live server availability
badges (Online / Offline), quick-action links to the native Windows print dialog, and a prominent action
to launch the connection wizard.

### 4. Component Standardization & Responsive Layout

All panels are refactored to standard Tailwind and shadcn-ui primitives (`Button`, `Card`, `Badge`,
`Checkbox`, `Input`, `Dialog`):
- Navigation collapses from a full sidebar to a 64px icon rail when viewport width drops below 860px.
- Forms and action bars wrap naturally on small screens without truncation.
- Print problems are presented as contextual recovery banners only when an active failure occurs,
  leaving the workspace clear when healthy.

## Consequences

- End users can discover, trust, and install shared printers in a single cohesive flow without needing
  to understand backend daemon lifecycles.
- System diagnostics remain 1 click away for troubleshooting and advanced administrators.
- The UI conforms cleanly to Windows desktop conventions and handles resizing gracefully.
