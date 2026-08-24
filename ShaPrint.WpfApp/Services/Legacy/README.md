# Legacy Services (Deprecated)

Folder ini berisi service-service yang **deprecated** dan akan dihapus di versi berikutnya.

## Deprecated Services

### DriverPackageManager
- **Status**: Deprecated
- **Replacement**: IPP server handles all printing
- **Reason**: Client tidak perlu download driver lagi

### DriverInstaller
- **Status**: Deprecated
- **Replacement**: IPP server handles all printing
- **Reason**: Client tidak perlu install driver lagi

### VirtualPrinterManager
- **Status**: Deprecated
- **Replacement**: IPP printer (Windows built-in)
- **Reason**: Client tidak perlu virtual printer lagi

### PipeListener
- **Status**: Deprecated
- **Replacement**: IPP protocol
- **Reason**: Tidak perlu named pipe interception

## Migration Path

1. **Phase 1** (Current): IPP server implemented, legacy still works
2. **Phase 2** (Next): Legacy code marked deprecated, warnings shown
3. **Phase 3** (Future): Legacy code removed

## How to Remove

Ketika siap untuk menghapus legacy code:

1. Hapus folder `Legacy/`
2. Hapus reference ke deprecated services di `ServerViewModel`
3. Hapus `DriverPackageService`, `DriverPackageManager`, `DriverInstaller`, `VirtualPrinterManager`, `PipeListener`
4. Update UI untuk menghapus legacy options
