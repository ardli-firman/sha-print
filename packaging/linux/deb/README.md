# ShaPrint DEB Package Guide (Ubuntu & Debian)

This directory contains packaging specifications and build automation to produce native `.deb` packages for **Ubuntu 22.04 LTS**, **Ubuntu 24.04 LTS**, **Debian 11 (Bullseye)**, and **Debian 12 (Bookworm)**.

---

## 📋 Package Overview

- **Package Name**: `shaprint`
- **Architectures**: `amd64`
- **Installed Binaries**:
  - `/usr/bin/shaprintd` — System daemon (runs under unprivileged `shaprint` user)
  - `/usr/bin/shaprintctl` — Administrative CLI tool
- **System Integration**:
  - `systemd` service: `/usr/lib/systemd/system/shaprintd.service`
  - System user: `/usr/lib/sysusers.d/shaprint.conf`
  - Tmpfiles: `/usr/lib/tmpfiles.d/shaprint.conf`
  - System preset: `/usr/lib/systemd/system-preset/90-shaprint.preset`
  - Configuration directory: `/etc/shaprint`
  - Root-protected credential storage: `/etc/shaprint-credentials`
  - Shared state directory: `/var/lib/shaprint`
  - Polkit policy: `/usr/share/polkit-1/actions/org.shaprint.control.policy`

---

## 🛠️ Building the DEB Package

### Prerequisites
- Tools: `bash`, `dpkg-deb`, `cargo`, `rust`, `gcc`
- Optional: `podman` or `docker` (used automatically if building on a host with a glibc baseline > 2.31 to preserve backward compatibility with Debian 11 / Ubuntu 22.04).

### Build Command
Run from the repository root:

```bash
# Build DEB for current distribution
make build-deb

# Or invoke the build script directly
bash packaging/linux/deb/build-deb.sh
```

Built artifacts are output to `apps/desktop/src-tauri/target/deb/`:
```text
apps/desktop/src-tauri/target/deb/shaprint_3.4.0-1_amd64.deb
```

---

## 📥 Installation

Install the generated package using `apt` or `dpkg`:

```bash
# Using APT (recommended, installs dependencies automatically)
sudo apt install ./apps/desktop/src-tauri/target/deb/shaprint_3.4.0-1_amd64.deb

# Or using dpkg directly
sudo dpkg -i ./apps/desktop/src-tauri/target/deb/shaprint_3.4.0-1_amd64.deb
sudo apt-get install -f # Fix missing dependencies if needed
```

The package automatically:
1. Creates the unprivileged `shaprint` system user and group.
2. Provisions directories with strict permissions (`0750` for config, `0700` for state).
3. Enables and starts `shaprintd.service` via `systemctl`.
4. Automatically provisions UFW firewall rules if UFW is active.

---

## 🛡️ Firewall Configuration (`ufw`)

ShaPrint communicates on dedicated high ports:
- **TCP `48631`**: IPPS Print Sharing Server
- **UDP `48633`**: Multicast DNS Discovery Responder

If UFW is installed and active:

```bash
sudo ufw allow 48631/tcp comment "ShaPrint IPPS Server"
sudo ufw allow 48633/udp comment "ShaPrint mDNS Discovery"
sudo ufw reload
```

---

## 🚀 Quick Start with `shaprintctl`

For complete tutorials and workflow documentation, see the [ShaPrint Linux Guide](../README.md).

```bash
# 1. Verify daemon readiness
shaprintctl status

# 2. Configure Network Channel password
sudo shaprintctl network-channel set

# 3. Connect to a remote printer (Client Role)
shaprintctl inspect 192.168.1.105:48631
sudo shaprintctl trust approve 192.168.1.105:48631 <FINGERPRINT>
shaprintctl printers 192.168.1.105:48631
# Gunakan tanda petik jika nama printer mengandung spasi / tanda kurung
sudo shaprintctl queue install 192.168.1.105:48631 "<PRINTER_NAME>"

# 4. Share local printers to the network (Server Role)
shaprintctl sharing list
sudo shaprintctl sharing set <PRINTER_NAME>
sudo shaprintctl start server-sharing
```

---

## 🧪 Smoke Testing

Run the automated containerized smoke test suite across all 4 supported distribution targets (requires `podman`):

```bash
# Ubuntu 24.04 LTS & 22.04 LTS
make smoke-deb-ubuntu2404
make smoke-deb-ubuntu2204

# Debian 12 (Bookworm) & Debian 11 (Bullseye)
make smoke-deb-debian12
make smoke-deb-debian11
```
