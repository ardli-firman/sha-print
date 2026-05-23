# ShaPrint - LAN & Cross-VLAN Virtual Printer Sharing

**ShaPrint** adalah solusi modern berbasis .NET 8 untuk melakukan *sharing printer* di jaringan lokal (LAN) maupun lintas-VLAN (Subnet). Aplikasi ini menjadi solusi jitu ketika Anda tidak bisa menggunakan fitur bawaan Windows SMB (Shared Folder/Printer), sering terkendala masalah *password/credential* jaringan, atau terhalang oleh aturan sekuritas Windows 10/11.

Sistem ini menggunakan arsitektur **Virtual Printer Driver (Named Pipes)** dan protokol TCP/UDP murni, dengan kualitas hasil cetak dokumen 100% sempurna (*Native Driver Approach*).

---

## ✨ Fitur Utama

- **Unified Application:** Hanya butuh 1 aplikasi `.exe` yang sama. Anda dapat memilih bertindak sebagai **Server** (tuan rumah printer fisik) atau **Client** (pengirim dokumen).
- **Native Driver Quality:** Menggunakan driver printer asli (misal: Epson L3210) di sisi Client, sehingga kualitas hasil cetak dokumen (kertas, warna, margin) akan 100% sama dengan aslinya (bukan kualitas *Generic/Text* atau cetak PDF terdegradasi).
- **Cross-VLAN Support:** Dilengkapi fitur *Specific Server IP* untuk menerobos blokade router jika PC Client dan PC Server berada di subnet / VLAN IP yang berbeda.
- **In-App Realtime Logging:** Dilengkapi panel Terminal Log "ala Hacker" langsung di dalam aplikasi (UI) untuk proses *debugging* aliran data.
- **Background Service:** Aplikasi dapat di-minimize ke *System Tray* dan berjalan diam-diam melayani pekerjaan cetak (*print jobs*).

---

## 🏗 Arsitektur Sistem

1. **Server Mode**
   Aplikasi berjalan di komputer yang terhubung langsung ke printer fisik (via kabel USB). Server membaca daftar printer lokal, dan bersiap menerima data cetak mentah (*raw spool data*) dari jaringan via **TCP Port 9877**. Server juga memancarkan sinyal pendeteksian di **UDP Port 9876**.
   
2. **Client Mode**
   Aplikasi membuat *Virtual Printer Port* di Windows. Semua proses cetak dari Microsoft Word / PDF / Web Browser ke printer ini akan ditangkap oleh aplikasi Client dan dialirkan langsung menuju Server melalui jaringan, tanpa delay BIDI (*Bidirectional*).

---

## 💻 Cara Instalasi

Anda tidak perlu menginstal .NET Framework. Aplikasi ini sudah dikemas utuh dalam sebuah Installer (*Standalone Setup*).

1. Buka folder `Output` (jika Anda mem-build sendiri) atau minta file Installer dari administrator Anda.
2. Jalankan `ShaPrint_Setup_v1.0.exe`.
3. Aplikasi akan otomatis membuat *shortcut* di Desktop dan Start Menu Anda.

---

## 🛠 Panduan Penggunaan

**Syarat Mutlak (Native Driver):** Agar dokumen tidak rusak saat melintasi jaringan, pastikan Anda **menginstal Driver Resmi Printer tersebut** di PC Client (misal: install *Driver Epson L3210* di PC Client, meskipun printernya tidak dicolok fisik ke PC Client).

### 1. Di Komputer Server (Yang dicolok USB Printer)
- Buka ShaPrint dari Desktop Anda.
- Saat pertama kali muncul dialog, pilih **"Run as Server"**.
- Centang printer fisik yang ingin dibagikan ke jaringan.
- Klik **"Start Server"**.
- Selesai! Anda bisa me-*minimize* aplikasi ini ke sudut bawah layar (*System Tray*).

### 2. Di Komputer Client (Yang ingin numpang nge-print)
- Buka ShaPrint dari Desktop Anda (**Wajib Run as Administrator** jika ini pertama kali, karena Windows butuh akses admin untuk mendaftarkan printer baru).
- Saat ditanya mode, pilih **"Run as Client"**.
- Jika Anda 1 WiFi / 1 Switch dengan Server: Klik **"Scan LAN for Printers"**.
- Jika Anda berbeda VLAN / Gedung dengan Server: Ketik IP Server di kotak "Specific Server IP" lalu tekan Enter/Scan.
- Pilih printer yang muncul di daftar, lalu klik **"Install Selected Printer"**.
- Buka Microsoft Word, tekan `Ctrl + P`, pilih printer `ShaPrint - [Nama Printer]`, dan **Print!**

---

## ⚙️ Build ke Production (Developer)

Jika Anda ingin membungkus (*compile*) kode sumber dan membuat Installer sendiri:

### 1. Compile Aplikasi (Single File)
Buka Terminal di folder proyek `shaprint` dan ketikkan:
```bash
dotnet publish ShaPrint.App/ShaPrint.App.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true
```

### 2. Buat Installer Windows
Aplikasi ini sudah dikonfigurasi untuk dibungkus menggunakan **Inno Setup 6**.
Jika Anda sudah menginstal Inno Setup, buka *PowerShell* dan ketik:
```powershell
& 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe' installer.iss
```
File Installer `ShaPrint_Setup_v1.0.exe` akan muncul di dalam folder `Output\`.

---

## ⚠ Pemecahan Masalah (Troubleshooting)

- **Failed to install printer (Client):** Pastikan Anda menekan klik kanan -> *Run as Administrator* saat membuka ShaPrint. Windows butuh *permission* untuk menanamkan virtual printer.
- **Word nge-hang / Connecting to Printer lama sekali:** Ini biasanya karena fitur BIDI (*Bidirectional Support*) Windows aktif. ShaPrint sudah otomatis mematikan BIDI saat instalasi, namun jika masih terjadi, buka Control Panel -> Devices & Printers -> Klik Kanan Printer ShaPrint -> Printer Properties -> tab *Ports* -> Hapus centang pada *Enable bidirectional support*.
- **Client tidak bisa menemukan Server (Scan kosong):** Matikan Windows Firewall di PC Server, atau gunakan kolom "Specific Server IP" jika Client & Server berada di segmen IP yang berbeda (Beda VLAN).
- **Hasil Print Error / Keluar Huruf Acak:** Ini terjadi karena Driver Printer di PC Client tidak sama dengan PC Server. Pastikan menginstal Driver Resmi yang sama di kedua belah pihak.
