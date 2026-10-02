# Login startup and the tray lifecycle

---
status: accepted
---

ShaPrint runs for the whole signed-in session, not for as long as its window happens to be open. It
registers itself for the current user's Windows login under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` with a `--background` argument, starts with no
window in that mode, and closing its window hides it to the notification area instead of ending it.
An explicit **Quit ShaPrint** in the tray menu stops the background services and exits.

## Why it works this way

- Installed print queues address the loopback client proxy at any moment, so the proxy has to be
  running whether or not a window is open (#34, ADR 0001). Closing the window therefore must not
  stop it, and an explicit quit must stop everything cleanly.
- The registration lives under the current user, so it needs no administrator rights and the app's
  normal operation stays unprivileged. The one elevated action remains the firewall rule (ADR 0003).
- Hiding to the tray is only safe while a tray icon exists to bring the window back and to quit
  from. When the tray cannot be created — no bundled icon, or the platform refuses one — closing the
  window ends the app, because a running process the user cannot reach is worse than a stopped one.
- Server sharing keeps its explicit Start/Stop control. Nothing in the login path starts sharing, so
  a login launch shares nothing the user did not ask to share.
- The product default — start with Windows — is applied **once**, on a computer where the user has
  never made a choice. The window can then turn it on or off, and the choice is recorded in the app
  data directory. A launch after that leaves the registration exactly as the user left it, because a
  preference that a later start silently restores is not a preference. An unreadable choice record
  counts as a choice, so a launch never re-registers over something the user may have turned off.

## Consequences

- The main window is created hidden and shown by the shell, so a login launch never flashes a window
  and an interactive launch still opens it.
- The tray menu is the only quit path while a tray is available, so the window has to say so.
- Starting a second copy while one is running competes for the loopback proxy port and the sharing
  port. The proxy reports the conflict as a recoverable failure for the user to act on; single
  instance handling is not part of this decision.
