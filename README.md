<div align="center">
  <h1>🖨️ ShaPrint</h1>
  <p><b>A modern, lightning-fast LAN & Cross-VLAN Virtual Printer Sharing Solution for Windows</b></p>
</div>

---

**ShaPrint** is a modern .NET 8-based solution for sharing physical printers across local networks (LAN) and across subnets/VLANs. It serves as the perfect workaround when native Windows SMB (Shared Folder/Printer) fails you, when you are haunted by network credential issues, or blocked by strict Windows 10/11 security policies.

By utilizing a **Virtual Printer Driver (Named Pipes)** architecture and raw TCP/UDP protocols, ShaPrint guarantees that your documents are printed with **100% fidelity** and native quality.

---

## ✨ Key Features

- 🎭 **Unified Application:** One executable rules them all. Choose to run it as a **Server** (hosting the physical printer) or a **Client** (sending the documents) from the exact same `.exe`.
- 💎 **Native Driver Quality:** Unlike traditional workarounds that degrade quality to *Generic/Text* or PDF rendering, ShaPrint uses the actual printer driver (e.g., Epson L3210) on the Client side. What you see is exactly what you print—perfect margins, perfect colors.
- 🌍 **Cross-VLAN Support:** Network segmented? No problem. Use the *Specific Server IP* feature to bypass router boundaries and connect a Client to a Server sitting in a completely different subnet or VLAN.
- ⚡ **Seamless Auto-Updater:** ShaPrint comes bundled with a dedicated background updater. It checks for new releases on GitHub at startup and updates itself seamlessly on-the-fly without interrupting your active print jobs.
- 🧑‍💻 **In-App Realtime Logging:** A clean, readable built-in terminal log panel lets you monitor the data flow and debug network issues easily directly from the UI.
- 👻 **Stealth Background Service:** Minimize the app to the System Tray and let it serve your print jobs silently in the background. Starts automatically with Windows!

---

## 🏗 System Architecture

1. **Server Mode**
   The application runs on the computer directly connected to the physical printer via USB. It scans for local printers and listens for raw print spool data from the network on **TCP Port 9877**. It also broadcasts its presence on **UDP Port 9876** for auto-discovery.
   
2. **Client Mode**
   The application creates a *Virtual Printer Port* in Windows. Any document printed from Microsoft Word, PDF readers, or Web Browsers to this virtual printer is instantly intercepted by the Client and streamed directly to the Server over the network, completely eliminating Bidirectional (BIDI) delay issues.

---

## 🚀 Installation

You don't need to install the .NET Framework beforehand. ShaPrint comes packaged as a fully self-contained Standalone Setup!

1. Download `ShaPrint_Setup_vX.Y.Z.exe` from the GitHub Releases page (or get it from the `Output` folder if you built it yourself).
2. Run the installer.
3. The application will automatically create shortcuts on your Desktop and Start Menu.

---

## 📖 How to Use

> [!IMPORTANT]  
> **Native Driver Requirement:** To ensure 100% print fidelity, you **must install the official driver for the printer on the Client PC** (e.g., install the Epson L3210 driver on the Client, even though the printer is physically plugged into the Server).

### 1. On the Server PC (Plugged into the Printer)
- Open ShaPrint from your Desktop.
- On the first launch, select **"Run as Server"**.
- Check the boxes next to the physical printers you want to expose to the network.
- Click **"Start Server"**.
- You're done! You can minimize the app to the System Tray.

### 2. On the Client PC (Sending the Print Jobs)
- **Run ShaPrint as Administrator** (Windows requires admin privileges to register the virtual printer port).
- Select **"Run as Client"**.
- If you are on the same Wi-Fi / Switch as the Server: Click **"Scan LAN / Connect"**.
- If you are on a different VLAN / Building: Type the Server's IP address into the "Specific Server IP" box and click Scan.
- Select your target printer from the list and click **"Install Selected Printer"**.
- Open Microsoft Word (or any app), press `Ctrl + P`, select `ShaPrint - [Printer Name]`, and **Print!**

---

## ⚙️ Building from Source

If you want to compile the source code and generate the installer yourself:

### 1. Compile the Application
Open a terminal in the `shaprint` root directory and run:
```bash
dotnet publish ShaPrint.App/ShaPrint.App.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true

# Do the same for the Updater
dotnet publish ShaPrint.Updater/ShaPrint.Updater.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true
```

### 2. Build the Windows Installer
The application is configured to be packaged using **Inno Setup 6**. Open PowerShell and compile the `.iss` script:
```powershell
& 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe' installer.iss
```
Your compiled installer `ShaPrint_Setup_v1.0.0.exe` will be generated inside the `Output\` folder.

---

## 🛠 Troubleshooting

- **Failed to install printer (Client):** Ensure you opened ShaPrint via Right-Click -> *Run as Administrator*. The application needs elevated permissions to inject the virtual printer into the Windows spooler.
- **Word freezes / "Connecting to Printer" takes forever:** This happens if Windows Bidirectional Support (BIDI) is enabled. ShaPrint disables this during installation, but if it persists: go to Control Panel -> Devices & Printers -> Right-click the ShaPrint Printer -> Printer Properties -> *Ports* tab -> Uncheck *Enable bidirectional support*.
- **Client cannot find the Server (Empty scan list):** Disable the Windows Firewall on the Server PC or ensure ports 9876/UDP and 9877/TCP are open. If you are on a different subnet, auto-discovery won't work—use the "Specific Server IP" box instead.
- **Printed output is gibberish/error codes:** This occurs when the Printer Driver on the Client PC does not match the Server PC. Make sure you install the exact same Official Driver on both machines.

---

<div align="center">
  <b>Built with ❤️ by ardli-firman</b>
</div>
