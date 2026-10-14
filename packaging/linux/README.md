# ShaPrint on Linux — Headless Daemon & `shaprintctl` CLI Guide

ShaPrint on Linux runs as a native, headless, machine-wide system service (`shaprintd`) integrated with systemd, local CUPS print spooling, and host firewalls (`firewalld` / `ufw`). It is fully managed and operated through the `shaprintctl` CLI tool over a protected local Unix domain socket (`/run/shaprint/shaprintd.sock`).

---

## 🏗️ Architecture & Philosophy

- **System Daemon (`shaprintd`)**: Runs under an unprivileged `shaprint` system user. Manages the IPPS server runtime (TCP `48631`), client loopback proxy (TCP `48632`), and background mDNS discovery (UDP `48633`).
- **Control CLI (`shaprintctl`)**: Communicates with the daemon over `/run/shaprint/shaprintd.sock`. Allows unprivileged status queries and requires root / `sudo` (or polkit) for state-altering administrative commands.
- **CUPS Integration**: Native Linux CUPS queues are automatically created for remote printers, pointing through loopback proxy `127.0.0.1:48632`. Any Linux desktop application (LibreOffice, Chrome, Firefox, GIMP) or command (`lp`, `lpr`) can print transparently.
- **No Plaintext Secrets**: Network Channels are never passed as command-line arguments. Secret input is handled via interactive hidden prompts, `--stdin`, `--fd`, or root-protected credential files.

---

## 🔍 How Discovery Works in CLI

Users often wonder: **"Where is `shaprintctl discovery`?"**

In ShaPrint Linux architecture:
1. **Continuous Background mDNS**: The `shaprintd` daemon continuously listens and advertises on multicast DNS (`_shaprint-ipps._tcp.local.`, UDP `48633`) in the background across all non-loopback network interfaces.
2. **Explicit Zero-Trust Inspection**: Rather than dumping unverified network broadcasts into print queues, connecting to a server (whether on the local subnet or across VLANs) uses the deterministic **Trust-on-First-Use (TOFU)** workflow:
   - **Step 1:** Inspect the target server's IPPS endpoint:
     ```bash
     shaprintctl inspect <server-ip>:48631
     ```
   - **Step 2:** Review and approve the TLS certificate fingerprint:
     ```bash
     sudo shaprintctl trust approve <server-ip>:48631 <fingerprint>
     ```
   - **Step 3:** List the server's shared printers:
     ```bash
     shaprintctl printers <server-ip>:48631
     ```

This works identically for both **same-subnet mDNS discovery** and **cross-VLAN routing** without needing multicast repeaters.

---

## 📖 `shaprintctl` Command Reference & Tutorial

### 1. Daemon & Service Status

Check operational readiness and active subsystems:

```bash
# Human-readable status overview
shaprintctl status

# Machine-readable JSON output
shaprintctl --json status

# Comprehensive 6-subsystem diagnostic audit (daemon, ports, CUPS, trust, credentials, firewall)
shaprintctl diagnostics
# (or shorthand: shaprintctl diag)
```

Start or stop specific runtimes independently:

```bash
# Start or stop the Client Proxy runtime (TCP 48632)
sudo shaprintctl start client-proxy
sudo shaprintctl stop client-proxy

# Start or stop the Server Sharing runtime (TCP 48631 & UDP 48633)
sudo shaprintctl start server-sharing
sudo shaprintctl stop server-sharing

# Stop the entire shaprintd daemon cleanly
sudo shaprintctl stop daemon
```

---

### 2. Network Channel Setup (Authentication)

A matching **Network Channel** password must be configured between servers and clients.

> ⚠️ **Security Rule:** Never pass passwords inline as CLI flags. `shaprintctl` strictly rejects inline secret arguments.

```bash
# 1. Interactive hidden prompt (recommended for human admins)
sudo shaprintctl network-channel set

# 2. Via STDIN (ideal for scripts and deployment automation)
echo -n "YourSecretChannelKey" | sudo shaprintctl network-channel set --stdin

# 3. Via a secure root-protected file
sudo shaprintctl network-channel set --credential-file /root/channel.key

# Verify configuration status (displays whether configured, never shows the secret)
shaprintctl network-channel status
```

---

### 3. Discovering, Trusting & Connecting to Remote Printers (Client Role)

To connect your Linux machine to a ShaPrint server (Windows Desktop, Android, or another Linux server):

#### Step 1: Inspect the Server
Query the remote IPPS endpoint to retrieve its identity and TLS SHA-256 fingerprint:

```bash
shaprintctl inspect 192.168.1.105:48631
```
*Output:*
```text
address: 192.168.1.105:48631
fingerprint: E9A4B1288C033D17F4A92B6011495A78B819D2FE48792015AB20D8478A9C1204
trusted: false
```

#### Step 2: Approve the Server (Pin TLS Certificate)
Approve the certificate fingerprint to establish encrypted, tamper-proof communication:

```bash
sudo shaprintctl trust approve 192.168.1.105:48631 E9A4B1288C033D17F4A92B6011495A78B819D2FE48792015AB20D8478A9C1204
```

Verify your trusted servers list:
```bash
shaprintctl trust list
```

#### Step 3: List Shared Printers on the Remote Server
```bash
shaprintctl printers 192.168.1.105:48631
```
*Example output:*
```text
EPSON_L3210_Series
HP_LaserJet_1020
```

#### Step 4: Install Native CUPS Print Queue
Install the remote printer into your local Linux CUPS system:

> 💡 **Tip:** Jika nama printer memiliki spasi atau tanda kurung seperti `(Copy 1)`, **selalu bungkus dengan tanda petik ganda (`"..."`)** agar tidak dianggap sebagai operator shell/subshell oleh Bash:

```bash
# Untuk nama printer tanpa spasi
sudo shaprintctl queue install 192.168.1.105:48631 EPSON_L3210_Series

# Untuk nama printer dengan spasi atau tanda kurung (Copy 1)
sudo shaprintctl queue install 10.102.10.21:48631 "EPSON L3210 Series (Copy 1)"
```

List installed client queues:
```bash
shaprintctl queue list
```

Now print from anywhere!
```bash
# Print a test file via CUPS
lp -d shaprint_EPSON_L3210_Series /path/to/document.pdf
```

#### Queue Repair
If printer capabilities or paper sizes change on the remote host, refresh the queue without deleting it:
```bash
sudo shaprintctl queue repair 192.168.1.105:48631 EPSON_L3210_Series
```

---

### 4. Sharing Local Printers to the Network (Server Role)

To share physical printers connected to this Linux machine with other Windows, Android, and Linux devices:

#### Step 1: List Local CUPS Printers
```bash
shaprintctl sharing list
```
*Example output:*
```text
Brother_HL-L2350DW: unshared
Canon_TS300_Series: unshared
```

#### Step 2: Share Desired Printers
```bash
# Share one or more printers
sudo shaprintctl sharing set Brother_HL-L2350DW Canon_TS300_Series

# Verify sharing status
shaprintctl sharing list

# Start the server sharing runtime if not already active
sudo shaprintctl start server-sharing
```

To stop sharing all printers:
```bash
sudo shaprintctl sharing clear
```

---

### 5. Maintenance, Job Draining & Purge

#### Graceful Job Draining Before Maintenance
Before updating the package or restarting the server, ensure all spooling jobs finish:

```bash
# Wait up to 30 seconds for active print jobs to complete
shaprintctl drain --timeout 30
```

#### Complete Machine Purge
To cleanly uninstall ShaPrint and remove all created CUPS queues, firewall rules, user state, and credentials:

```bash
sudo shaprintctl purge --confirm
```

---

## 📦 Package Guides

- [🐧 AlmaLinux / RHEL / Fedora (RPM Guide)](rpm/README.md)
- [🐧 Ubuntu / Debian (DEB Guide)](deb/README.md)
