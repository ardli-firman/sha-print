# Windows-first IPP print sharing

---
status: accepted
---

ShaPrint will replace the WPF application with a Tauri shell and Rust services. The MVP targets Windows 10 and later and supports Windows clients printing to Windows servers through native print queues, a local Rust client proxy, and a Rust IPPS server that forwards jobs to existing Windows printer queues. The proxy attaches the Network Channel credentials, while IPPS uses explicit certificate fingerprint approval on first use; printing must preserve visual output and common settings such as media, color, duplex, and copies, but does not promise byte-identical rendered data. A later phase will add Linux clients and servers, initially targeting AlmaLinux and using CUPS for IPP sharing.

## Consequences

- Windows clients and servers discover printers through mDNS or a manually entered server address. Linux will use CUPS sharing in the later phase.
- The app starts at login and remains in the system tray. Client proxies and Windows IPPS sharing run while it is active; server sharing is controlled by an explicit Start/Stop setting. The proxy must be running for client print jobs to reach a server.
- Setup may request administrator permission once; normal app operation uses the logged-in user. The later AlmaLinux phase will configure a dedicated CUPS account whose password is the Network Channel and may request root permission during setup.
- The Windows MVP includes print sharing only. The new app uses IPP and does not implement the old TCP/named-pipe print path. The existing WPF app remains a separate transition path.
- On Windows, the app imports existing ShaPrint settings but asks the user to select shared printer queues again. AlmaLinux starts with new configuration in the later phase.
- PR #27 informed the design but is not the implementation base: it uses unencrypted HTTP and a Windows spooler adapter; its Linux console uses an in-memory spooler rather than CUPS.
