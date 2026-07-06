# Server Monitoring Panel — Design Spec

## Problem

ShaPrint digunakan di lingkungan dengan puluhan ruangan, setiap ruangan memiliki 1 server + beberapa client. Admin (Network Server Printer Administrator) tidak memiliki visibilitas ke status seluruh server dari satu tempat. Saat ini:

- **Server mode:** hanya lihat status server sendiri, tidak bisa lihat server ruang lain
- **Client mode:** bisa cetak & scan ke server, tidak ada panel monitoring multi-server
- **Log hanya tekstual** — tidak ada dashboard visual dengan status warna

Tidak ada cara bagi admin untuk memonitor kesehatan printer, scanner, client, dan job history dari **semua server** di jaringan LAN secara real-time.

## Keputusan

### Mode Baru: "Monitor" Mode

Mode ketiga (setelah Server dan Client) di WelcomePage. Dedicated untuk administrator:

- Tidak host printer atau scanner
- Tidak perlu Virtual Printer Driver
- Startup ringan — tidak load PrintReceiver, ScannerService, Spooler
- Sidebar: hanya **Server Monitoring** + Settings + Updates
- Bisa di PC khusus operator (ruang IT), tanpa harus stop server atau ganti mode

**Phase 2 (future):** Tambah nav item "Server Monitoring" di sidebar Server & Client mode agar operator ruangan bisa lihat sekilas tanpa ganti mode.

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Monitor Mode PC                          │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  MonitorPage.xaml  (grid kartu + detail expand)       │   │
│  │  MonitorViewModel  (ObservableCollection<ServerNode>) │   │
│  ├──────────────────────────────────────────────────────┤   │
│  │  MonitorService (background — TCP poll tiap 15s)     │   │
│  │  DiscoveryClient (UDP — daftar server discovery)     │   │
│  └──────────────────────────────────────────────────────┘   │
└───────────────────┬─────────────────────────────────────────┘
                    │ UDP broadcast (discovery)
                    │ TCP :9878 (GET_STATUS — new port)
        ┌───────────┼───────────┬───────────┐
        ▼           ▼           ▼           ▼
   Server A    Server B    Server C    Server D
   (Ruang 1)   (Ruang 2)   (Ruang 3)   (Ruang 4)

## Data Flow

### 1. Discovery (UDP — existing, unchanged)

- `DiscoveryServer` broadcast via UDP port 9876
- `DiscoveryResponseMessage` berisi: `ServerName` (hostname), `IpAddress`, `ExposedPrinters`, `ExposedScanners`
- Monitor mode jalankan `DiscoveryClient.DiscoverServersAsync()` secara periodik
- Dapatkan daftar semua server di jaringan

### 2. Status Collection (TCP — new `GET_STATUS` on port 9878)

PrintReceiver on port 9877 uses **binary protocol** (4-byte int header) — cannot be reused for text commands.
Server needs **new TcpListener on port 9878** for text-based monitor commands.

Monitor mode sends `GET_STATUS\n` via TCP. Server responds with JSON `ServerStatusPayload`, terminated by `\n`.

MonitorService polls every 15s, **staggered** (server 1 at t=0, server 2 at t=1s, etc) to avoid spike.
Uses TCP — reliable, no size limit.

## UI

### New Page: `MonitorPage.xaml`

**Tampilan utama:** Grid dengan kartu per-server (2-3 kolom responsif). Indikator warna di setiap kartu.

| Status | Warna | Kondisi |
|---|---|---|
| 🟢 Online | `SystemFillColorSuccessBrush` | Semua normal |
| 🟡 Warning | `SystemFillColorCautionBrush` | Toner low, queue >5, scanner error |
| 🔴 Offline | `SystemFillColorCriticalBrush` | Printer error (paper jam) / no response >30s |
| ⚪ Unknown | `TextFillColorDisabledBrush` | Discovery lihat, tapi belum pernah response |

**Isi kartu per-server:**
- Host name + IP
- Printer list + status
- Scanner list + status
- Active clients count
- Uptime
- Last seen

**Filter & Sort:**
- Textbox filter by host name / IP (substring match)
- Sort by status (offline first)
- **Phase 2 (future):** add "Network Group" config in Server Settings for group-based filtering

**Detail panel (expand inline di bawah kartu):**
- **Printers tab:** daftar printer + queue length + error description
- **Scanners tab:** status per scanner
- **Clients tab:** list IP + connected since
- **Activity tab:** recent job history (type, document, status, timestamp)
- **Error banner:** merah di atas detail — recent errors dari PrintMonitorService

**Error states:**
- **Loading:** skeleton/progress ring
- **No servers:** "No servers discovered. Make sure servers are running on the network."
- **Network error:** per-server "Cannot reach server. Check firewall / connection."
- **Filter no result:** "No servers matching your filter."

### Port: 9878 (new, dedicated for text commands)

PrintReceiver on port 9877 uses **binary protocol** (4-byte int header). Cannot be reused.
New TCP listener on port 9878 for `GET_STATUS` text command.

Request (Monitor → Server):
```
GET_STATUS\n
```

Response (Server → Monitor, JSON terminator `\n`):
```json
{
  "serverName": "SERVER-RUANG3",
  "hostName": "SERVER-RUANG3",
  "networkName": "GedungA",
  "version": "1.0.0.0",
  "uptimeSeconds": 45240,
  "printers": [
    {
      "name": "Epson L3210",
      "status": "online",
      "queueLength": 2,
      "errorDescription": null
    }
  ],
  "scanners": [
    {
      "name": "Epson DS-570W",
      "status": "available",
      "lastScanAgo": "5m"
    }
  ],
  "activeClients": [
    { "ip": "192.168.1.25", "connectedSince": "2026-07-06T08:00:00Z" }
  ],
  "recentJobs": [
    {
      "type": "print",
      "document": "Laporan Q2.docx",
      "printerName": "Epson L3210",
      "clientIp": "192.168.1.25",
      "status": "completed",
      "timestamp": "2026-07-06T09:15:00Z"
    }
  ],
  "errors": [
    {
      "source": "PrintMonitor",
      "message": "Paper jam detected on Epson L3210",
      "timestamp": "2026-07-06T09:10:00Z"
    }
  ]
}
```

### Data model: `ShaPrint.Core/Network/MonitorModels.cs`

Semua model dalam satu file:
- `ServerStatusPayload` — root
- `PrinterStatus` — name, status enum (Online/Error/Idle), queueLength, errorDescription
- `ScannerStatus` — name, status enum (Available/InUse/Error), lastScanAgo
- `ActiveClientInfo` — ip, connectedSince
- `JobHistoryEntry` — type (Print/Scan), document, printerName, clientIp, status (Completed/Failed/Printing), timestamp
- `ServerErrorEntry` — source, message, timestamp

## Server-Side Changes

### New: `ServerStatusProvider` + `MonitorTcpServer`

ServerStatusProvider collects data from existing services and builds `ServerStatusPayload`.
Methods:
- `BuildStatus()` — collect all data, return payload

MonitorTcpServer listens on port 9878 — simple text protocol:

```
Monitor → Server: GET_STATUS\n
Server → Monitor:  {json}\n
```

Implementation: TcpListener with background accept loop (same pattern as PrintReceiver).

### Existing services — sudah punya data yang dibutuhkan:

| Data | Source | Status |
|---|---|---|
| Connected clients IP | `DiscoveryServer._connectedClients` | Existing |
| Printer queue | `PrintMonitorService` (via Spooler API) | Existing |
| Scanner status | `ScannerService` | Existing |
| Printer error | `PrintMonitorService.CheckPrintQueues()` | Existing |
| Uptime | `ServerViewModel` (startup timestamp) | Existing |
| Server hostname | `Environment.MachineName` | Existing di Discovery |

## New Files & Changes

### New files (~8):

| Path | Type |
|---|---|
| `ShaPrint.Core/Network/MonitorModels.cs` | Model |
| `ShaPrint.WpfApp/Services/Monitor/MonitorService.cs` | Service |
| `ShaPrint.WpfApp/Services/Server/ServerStatusProvider.cs` | Service |
| `ShaPrint.WpfApp/ViewModels/Pages/MonitorViewModel.cs` | ViewModel |
| `ShaPrint.WpfApp/Views/Pages/MonitorPage.xaml` + `.cs` | View |
| `ShaPrint.WpfApp/ViewModels/Windows/MonitorWindowViewModel.cs` | ViewModel |
| `ShaPrint.WpfApp/Views/Windows/MonitorWindow.xaml` + `.cs` | View (window) |
| `ShaPrint.WpfApp/Services/Monitor/README.md` | Doc |

### Modified files:

| `Services/Server/MonitorTcpServer.cs` | NEW — listen port 9878 for GET_STATUS |
| `Services/Server/ServerStatusProvider.cs` | NEW — build status payload |
### DI Registration:

```csharp
// Monitor Mode
services.AddSingleton<MonitorService>();
services.AddTransient<MonitorPage>();
services.AddSingleton<MonitorViewModel>();
services.AddSingleton<MonitorWindow>();
services.AddSingleton<MonitorWindowViewModel>();
```

### Startup Logic (`App.xaml.cs`):

```
if (mode == "Monitor")
{
    // No printer, no scanner, no print receiver
    // Start DiscoveryClient + MonitorService
    // Show MonitorWindow (bisa hidden di tray)
}
```

## Testing

| Test | Scope |
|---|---|
| `ServerStatusProvider.BuildStatus()` | Verify JSON structure, all fields populated, error handling when services unavailable |
| `MonitorService` polling | Verify staggered timing, timeout handling, server offline detection (>30s no response) |
| `MonitorTcpServer GET_STATUS` | Verify GET_STATUS command returns valid JSON, non-monitor connections untouched |
| `DiscoveryResponseMessage` | Verify hostname included, HMAC still works |
| UI behavior | Verify status colors, empty/error/filter states, detail expand/collapse |

## Out of Scope (Phase 1)

- Embedded "Server Monitoring" nav item di Server/Client mode sidebar (phase 2)
- Client-side failure reporting (client gagal connect ke server — phase 2)
- Multi-page job history pagination (server-side)
- Notification dari Monitor mode (toast ketika server offline — future)
- Historical data storage / database (data hanya live polling, tidak ada history)

| Skenario | Behavior |
|---|---|
| Server mati total (no response >30s) | Status 🔴 Offline. Last seen timestamp membesar. |
| Server baru nyala tengah jalan | Discovery detect — muncul di grid dalam 15 detik. |
| Network terputus | Semua server jadi 🔴. Banner "Connection lost" di atas grid. |
| Banyak printer (10+) | GET_STATUS response lebih besar, tapi TCP tanpa batas. |
| Firewall blocking TCP 9878 | Server tetap kelihatan via UDP (🟢), detail tidak bisa di-load. |
| Server version mismatch | Field optional di JSON diabaikan (forward compat). |
