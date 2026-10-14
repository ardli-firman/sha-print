# ShaPrint RPM Package Guide (AlmaLinux 9 & 10 / RHEL)

This directory contains packaging specifications and build automation to produce signed, native `.rpm` packages for **AlmaLinux 9**, **AlmaLinux 10**, and compatible Enterprise Linux distributions (RHEL, Rocky Linux, Fedora).

---

## 📋 Package Overview

- **Package Name**: `shaprint`
- **Architectures**: `x86_64`
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

## 🛠️ Building the RPM Package

### Prerequisites
- Tools: `bash`, `rpm-build`, `cargo`, `rust`, `gcc`
- Optional: `podman` or `docker` (used automatically if cross-building for AlmaLinux 9 on a host with glibc > 2.34 to preserve glibc 2.34 compatibility).

### Build Command
Run from the repository root:

```bash
# Build RPM for current distribution
make build-rpm

# Or invoke the build script directly
bash packaging/linux/rpm/build-rpm.sh

# Target AlmaLinux 9 specifically (uses containerized glibc 2.34 baseline when needed)
ALMALINUX_MAJOR=9 bash packaging/linux/rpm/build-rpm.sh
```

Built artifacts are output to `apps/desktop/src-tauri/target/rpm/`:
```text
apps/desktop/src-tauri/target/rpm/shaprint-3.4.0-1.el10.x86_64.rpm
```

---

## 📥 Installation

Install the generated RPM using `dnf` or `rpm`:

```bash
# Using DNF (recommended, resolves dependencies automatically)
sudo dnf install ./apps/desktop/src-tauri/target/rpm/shaprint-3.4.0-1*.rpm

# Or using rpm directly
sudo rpm -Uvh ./apps/desktop/src-tauri/target/rpm/shaprint-3.4.0-1*.rpm
```

The package automatically:
1. Creates the unprivileged `shaprint` system user and group.
2. Provisions directories with strict permissions (`0750` for config, `0700` for state).
3. Enables and starts the `shaprintd.service` via `systemctl`.

---

## 🛡️ Firewall Configuration (`firewalld`)

ShaPrint uses dedicated network ports:
- **TCP `48631`**: IPPS Print Sharing Server
- **UDP `48633`**: Multicast DNS Discovery Responder

To allow other devices on the LAN or across VLANs to access shared printers:

```bash
sudo firewall-cmd --permanent --add-port=48631/tcp
sudo firewall-cmd --permanent --add-port=48633/udp
sudo firewall-cmd --reload
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
```

> 🔒 **Izin Administrasi CUPS:** Pastikan user daemon `shaprint` memiliki akses ke grup `sys` dan `lp` agar diizinkan membuat antrean di CUPS (`lpadmin`):
> ```bash
> sudo usermod -a -G sys,lp shaprint && sudo systemctl restart shaprintd
> ```

# 4. Share local printers to the network (Server Role)
shaprintctl sharing list
sudo shaprintctl sharing set <PRINTER_NAME>
sudo shaprintctl start server-sharing
```

---

## 🧪 Smoke Testing

Run the automated containerized smoke test suite (requires `podman`):

```bash
# Test on AlmaLinux 9
make smoke-rpm-almalinux9

# Test on AlmaLinux 10
make smoke-rpm-almalinux10
```
