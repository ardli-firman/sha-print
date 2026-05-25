using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using Microsoft.Win32;
using ShaPrint.Core;
using ShaPrint.Server;

namespace ShaPrint.Client
{
    /// <summary>
    /// Manages virtual printer installation/removal using Win32 APIs.
    /// PowerShell has been eliminated to prevent command injection (RCE).
    /// All input MUST be validated by <see cref="Validators"/> before calling these methods.
    /// </summary>
    public static class VirtualPrinterManager
    {
        [DllImport("winspool.drv", SetLastError = true, CharSet = CharSet.Auto)]
        private static extern IntPtr AddPrinter(string? pName, uint Level, IntPtr pPrinterInfo);

        [DllImport("winspool.drv", SetLastError = true)]
        private static extern bool DeletePrinter(IntPtr hPrinter);

        [DllImport("winspool.drv", SetLastError = true, CharSet = CharSet.Unicode)]
        private static extern bool XcvDataW(
            IntPtr hXcv,
            string pszDataName,
            IntPtr pInputData,
            uint cbInputData,
            IntPtr pOutputData,
            uint cbOutputData,
            out uint pcbOutputNeeded,
            out uint pdwStatus);

        // -----------------------------------------------------------------
        // Public API
        // -----------------------------------------------------------------

        public static async Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(
            string virtualPrinterName, string pipeName, string driverName)
        {
            return await Task.Run(() =>
            {
                try
                {
                    // 1. Add local port
                    bool portOk = AddLocalPort(pipeName);
                    if (!portOk)
                    {
                        // Port might already exist — check if that's the case
                        int lastErr = Marshal.GetLastWin32Error();
                        AppLogger.Log($"[CLIENT] AddPort returned error {lastErr} (may already exist). Continuing.");
                    }

                    // 2. Try native driver first
                    var (hPrinter, nativeErr) = AddPrinterWithDriver(virtualPrinterName, pipeName, driverName);

                    // 3. Fall back to Generic / Text Only if native driver fails
                    if (hPrinter == IntPtr.Zero && !IsGenericDriver(driverName))
                    {
                        AppLogger.Log($"[CLIENT] Warning: Failed to install with Native Driver '{driverName}' (Error: {nativeErr}). Falling back to 'Generic / Text Only'.");
                        var fallbackResult = AddPrinterWithDriver(virtualPrinterName, pipeName, "Generic / Text Only");
                        hPrinter = fallbackResult.hPrinter;
                        nativeErr = fallbackResult.err;
                    }

                    if (hPrinter == IntPtr.Zero)
                    {
                        return (false, $"AddPrinter failed with Win32 error {nativeErr}. Ensure you have the correct driver installed AND run as Administrator.");
                    }

                    SpoolerApi.ClosePrinter(hPrinter);

                    // 4. Disable BIDI to prevent Windows Spooler / Word hanging
                    AppLogger.Log("[CLIENT] Disabling Bidirectional Support on the Virtual Printer to prevent UI hanging...");
                    DisableBidirectionalSupport(virtualPrinterName);

                    return (true, string.Empty);
                }
                catch (Exception ex)
                {
                    return (false, "Exception: " + ex.Message);
                }
            });
        }

        public static async Task<bool> RemovePrinterAsync(string printerName, string pipeName)
        {
            return await Task.Run(() =>
            {
                try
                {
                    // Remove printer
                    IntPtr hPrinter;
                    if (SpoolerApi.OpenPrinter(printerName, out hPrinter, IntPtr.Zero))
                    {
                        DeletePrinter(hPrinter);
                        SpoolerApi.ClosePrinter(hPrinter);
                    }

                    // Remove port
                    DeleteLocalPort(pipeName);
                    return true;
                }
                catch (Exception ex)
                {
                    AppLogger.Error($"[CLIENT] Failed to remove printer '{printerName}': {ex.Message}");
                    return false;
                }
            });
        }

        public static bool CheckPrinterExists(string printerName)
        {
            IntPtr hPrinter;
            bool exists = SpoolerApi.OpenPrinter(printerName, out hPrinter, IntPtr.Zero);
            if (exists)
                SpoolerApi.ClosePrinter(hPrinter);
            return exists;
        }

        // -----------------------------------------------------------------
        // Port management via XcvData
        // -----------------------------------------------------------------

        private static IntPtr OpenXcvMonitor()
        {
            IntPtr hXcv;
            if (!SpoolerApi.OpenPrinter(",XcvMonitor Local Port", out hXcv, IntPtr.Zero))
            {
                int err = Marshal.GetLastWin32Error();
                AppLogger.Error($"[CLIENT] OpenPrinter for XcvMonitor failed: {err}");
                return IntPtr.Zero;
            }
            return hXcv;
        }

        private static bool AddLocalPort(string portName)
        {
            IntPtr hXcv = OpenXcvMonitor();
            if (hXcv == IntPtr.Zero)
                return false;

            try
            {
                byte[] data = Encoding.Unicode.GetBytes(portName + '\0');
                IntPtr pData = Marshal.AllocHGlobal(data.Length);
                try
                {
                    Marshal.Copy(data, 0, pData, data.Length);
                    uint needed, status;
                    bool ok = XcvDataW(hXcv, "AddPort", pData, (uint)data.Length,
                        IntPtr.Zero, 0, out needed, out status);
                    if (!ok)
                    {
                        int err = Marshal.GetLastWin32Error();
                        AppLogger.Log($"[CLIENT] XcvData AddPort '{portName}': error {err}, status {status}");
                    }
                    return ok;
                }
                finally
                {
                    Marshal.FreeHGlobal(pData);
                }
            }
            finally
            {
                SpoolerApi.ClosePrinter(hXcv);
            }
        }

        private static bool DeleteLocalPort(string portName)
        {
            IntPtr hXcv = OpenXcvMonitor();
            if (hXcv == IntPtr.Zero)
                return false;

            try
            {
                byte[] data = Encoding.Unicode.GetBytes(portName + '\0');
                IntPtr pData = Marshal.AllocHGlobal(data.Length);
                try
                {
                    Marshal.Copy(data, 0, pData, data.Length);
                    uint needed, status;
                    return XcvDataW(hXcv, "DeletePort", pData, (uint)data.Length,
                        IntPtr.Zero, 0, out needed, out status);
                }
                finally
                {
                    Marshal.FreeHGlobal(pData);
                }
            }
            finally
            {
                SpoolerApi.ClosePrinter(hXcv);
            }
        }

        // -----------------------------------------------------------------
        // Printer management
        // -----------------------------------------------------------------

        private static (IntPtr hPrinter, int err) AddPrinterWithDriver(string printerName, string portName, string driverName)
        {
            var pi2 = new SpoolerApi.PRINTER_INFO_2
            {
                pPrinterName = printerName,
                pPortName = portName,
                pDriverName = driverName,
                pPrintProcessor = "WinPrint",
                pDatatype = "RAW",
                // All other fields default to null / 0
            };

            int structSize = Marshal.SizeOf<SpoolerApi.PRINTER_INFO_2>();
            IntPtr ptr = Marshal.AllocHGlobal(structSize);
            try
            {
                Marshal.StructureToPtr(pi2, ptr, false);
                IntPtr hPrinter = AddPrinter(null, 2, ptr);
                int err = hPrinter == IntPtr.Zero ? Marshal.GetLastWin32Error() : 0;
                
                if (hPrinter == IntPtr.Zero)
                {
                    AppLogger.Log($"[CLIENT] AddPrinter '{printerName}' with driver '{driverName}' failed: {err}");
                }
                return (hPrinter, err);
            }
            finally
            {
                Marshal.DestroyStructure<SpoolerApi.PRINTER_INFO_2>(ptr);
                Marshal.FreeHGlobal(ptr);
            }
        }

        private static void DisableBidirectionalSupport(string printerName)
        {
            try
            {
                string regPath = $@"SYSTEM\CurrentControlSet\Control\Print\Printers\{printerName}";
                using (var key = Registry.LocalMachine.OpenSubKey(regPath, writable: true))
                {
                    if (key != null)
                    {
                        key.SetValue("EnableBIDI", 0, RegistryValueKind.DWord);
                        AppLogger.Log($"[CLIENT] BIDI disabled for '{printerName}'.");
                    }
                    else
                    {
                        AppLogger.Log($"[CLIENT] Could not open registry key to disable BIDI for '{printerName}'. Path: {regPath}");
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[CLIENT] Failed to disable BIDI via registry: {ex.Message}");
            }
        }

        private static bool IsGenericDriver(string driverName)
        {
            string n = driverName.Trim();
            return n.Equals("Generic / Text Only", StringComparison.OrdinalIgnoreCase)
                || n.Equals("Generic/Text Only", StringComparison.OrdinalIgnoreCase);
        }
    }
}
