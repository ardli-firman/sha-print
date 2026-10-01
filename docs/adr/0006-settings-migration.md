# Migrating settings from the .NET application

---
status: accepted
---

An existing Windows ShaPrint user can move to the desktop app without retyping the Network Channel.
The import reads the previous application's files under `%LOCALAPPDATA%\ShaPrint`, takes the Network
Channel where the user set one, and reports every other previous setting as deliberately left
behind.

## What is imported, and what is not

Only the Network Channel has an equivalent in the new app. It is imported exactly as if the user had
typed it: a fresh salt, a fresh verifier, and a DPAPI-protected copy for the local proxy. The
previous app's protection (DPAPI, current user, entropy `ShaPrint-DPAPI-Entropy`) is opened
read-only. A value Windows will not open for this user is reported as unreadable rather than
invented or guessed.

Everything else is left behind on purpose:

- A previous app that never set a channel used the well-known placeholder `DefaultChannel`, and it
  *persisted* that placeholder — protected, so it looks like a real secret in the settings file —
  whenever the user left the field blank. The placeholder is therefore not a channel: it is an
  absence of configuration, and the new app asks for one instead of carrying a value the old app
  itself flagged as weak into a fresh installation.
- Installed client queues and shared server queues are never activated. Printer queues are selected
  again in the new app, so a stale queue that points at a machine no longer sharing it cannot come
  back to life.
- Auto-update, update channel, auto-purge, and scan settings have no equivalent in the print-sharing
  MVP; the report names each one and the reason it stayed behind.
- No certificate approval is imported. A server is trusted only when the user approves its
  fingerprint in the new app, so migration can never silently trust an identity (ADR 0003).

## Idempotence and safety

The import only ever reads the previous application's files, so the old app keeps working unchanged
during the transition. It writes once: the first time it takes a channel it records that fact in
`legacy-import.json` and stores the channel in its own app data. Every later start, and every manual
re-run, finds nothing to do.

## Consequences

- A user who had configured a channel prints after signing in without retyping it.
- A user who relied on the legacy default has to set a channel before clients can print.
- A mixed network in which old clients still send the legacy default is not supported by the MVP.
