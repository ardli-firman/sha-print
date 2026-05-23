# ShaPrint - LAN Virtual Printer Sharing

**ShaPrint** adalah solusi modern berbasis .NET 8 untuk melakukan *sharing printer* di jaringan lokal (LAN) tanpa perlu berurusan dengan kerumitan otentikasi Windows SMB (Shared Folder/Printer), *password* jaringan, atau kendala *credential* yang sering terjadi pada Windows 10/11.

Sistem ini menggunakan arsitektur **Client-Server** dengan **Virtual Printer Driver (Named Pipes)** dan protokol TCP/UDP langsung.

---

## 🏗 Arsitektur Sistem

1. **ShaPrint.Server (Pusat Printer)**
   Aplikasi ini berjalan di komputer yang terhubung langsung ke printer fisik (via USB). Aplikasi ini akan membaca daftar printer yang ada, menawarkannya ke jaringan (via UDP Broadcast Port 9876), dan mendengarkan permintaan cetak mentah dari klien (via TCP Port 9877).
   
2. **ShaPrint.Client (Pengguna Jaringan)**
   Aplikasi ini berjalan di komputer lain di dalam jaringan yang sama. Saat pengguna menginstal printer via aplikasi ini, sistem akan membuat *Virtual Printer* lokal (menggunakan driver Windows bawaan) dan menyambungkannya ke *Named Pipe*. Semua cetakan dari Microsoft Word/Browser akan dialirkan ke *Named Pipe* ini, lalu dikirimkan langsung ke Server via TCP.

---

## 💻 Cara Menjalankan untuk Development

Jika Anda seorang *developer* dan ingin menguji atau mengubah kode sumber:

### Prasyarat
- [Unduh & Install .NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0)
- Sistem Operasi: Windows 10 atau Windows 11.

### Langkah-langkah
1. Buka Terminal (Command Prompt / PowerShell) di folder proyek `shaprint`.
2. **Jalankan Server:**
   ```bash
   dotnet run --project ShaPrint.Server
   ```
   *(Aplikasi Server akan otomatis meminta izin UAC untuk mendaftarkan Port ke Windows Firewall pada saat pertama kali dibuka).*

3. **Jalankan Client:**
   Buka terminal/tab baru dan jalankan:
   ```bash
   dotnet run --project ShaPrint.Client
   ```
   *(PENTING: Client sebaiknya dijalankan sebagai Administrator/Terminal Admin jika Anda berencana menginstal/menghapus Virtual Printer).*

---

## 📦 Cara Build ke Production (Bundling)

Untuk menggunakan aplikasi ini di lingkungan operasional kantor, Anda dapat mem-paketkan (bundle) aplikasi menjadi file `.exe` tunggal yang bisa di- *copy-paste* ke komputer manapun tanpa perlu menginstal .NET Runtime.

Buka Terminal di folder proyek dan jalankan perintah berikut:

### Build Server
```bash
dotnet publish ShaPrint.Server/ShaPrint.Server.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true
```
Lokasi file siap pakai: `ShaPrint.Server\bin\Release\net8.0-windows\win-x64\publish\ShaPrint.Server.exe`

### Build Client
```bash
dotnet publish ShaPrint.Client/ShaPrint.Client.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true
```
Lokasi file siap pakai: `ShaPrint.Client\bin\Release\net8.0-windows\win-x64\publish\ShaPrint.Client.exe`

> **Note:** Anda cukup mengambil file `ShaPrint.XXX.exe` dari folder `publish` tersebut dan membagikannya ke komputer yang membutuhkan.

---

## 🛠 Panduan Penggunaan

1. **Di Komputer Server:**
   - Jalankan `ShaPrint.Server.exe`.
   - Centang printer fisik yang ingin dibagikan ke jaringan.
   - Klik **"Start Server"**.
   - (Opsional) Anda dapat menyilang/menutup aplikasinya; aplikasi akan otomatis bersembunyi di System Tray dan tetap melayani permintaan *print*.

2. **Di Komputer Client:**
   - Jalankan `ShaPrint.Client.exe` **(Klik Kanan -> Run as Administrator)**.
   - Klik **"Scan LAN for Printers"**.
   - Pilih printer yang terdeteksi, lalu klik **"Install Selected Printer"**.
   - Selesai! Buka Microsoft Word atau Browser, tekan `Ctrl + P`, dan pilih printer yang berawalan `ShaPrint - [Nama Printer]`. Hasilnya akan otomatis keluar di komputer Server.

---

## ⚠ Pemecahan Masalah (Troubleshooting)

- **Failed to install printer (Client):** Pastikan Anda menekan *Run as Administrator* saat membuka `ShaPrint.Client.exe`. Windows membutuhkan hak akses untuk mendaftarkan printer port secara lokal.
- **Connection Timeout / Gagal Mengeprint:** Pastikan komputer Server sudah mengizinkan `ShaPrint.Server.exe` untuk melewati *Windows Firewall*. (Biasanya aplikasi Server akan otomatis memunculkan _popup_ konfirmasi untuk membuka _Firewall_ saat pertama kali dijalankan).
- **Aplikasi tidak bisa dibuka (sudah berjalan):** Keduanya (Server & Client) memiliki fitur _Single Instance_ dan berjalan di latar belakang (_System Tray_). Periksa sudut kanan bawah layar Anda (dekat ikon WiFi/Baterai), klik kanan ikon ShaPrint, lalu pilih "Exit" jika ingin mematikannya secara total.
