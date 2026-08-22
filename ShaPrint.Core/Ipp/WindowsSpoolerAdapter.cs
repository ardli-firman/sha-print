using System.Runtime.InteropServices;

namespace ShaPrint.Core.Ipp;

/// <summary>
/// Production adapter: writes to Windows spooler via Win32 API.
/// Implements ISpoolerAdapter for the IPP server.
/// </summary>
public class WindowsSpoolerAdapter : ISpoolerAdapter
{
    // Win32 API declarations
    [DllImport("winspool.Drv", EntryPoint = "OpenPrinter", SetLastError = true, CharSet = CharSet.Auto)]
    private static extern bool OpenPrinter(string szPrinter, out IntPtr hPrinter, IntPtr pd);

    [DllImport("winspool.Drv", EntryPoint = "ClosePrinter", SetLastError = true)]
    private static extern bool ClosePrinter(IntPtr hPrinter);

    [DllImport("winspool.Drv", EntryPoint = "StartDocPrinter", SetLastError = true, CharSet = CharSet.Auto)]
    private static extern int StartDocPrinter(IntPtr hPrinter, int level, [In, MarshalAs(UnmanagedType.LPStruct)] DOCINFO di);

    [DllImport("winspool.Drv", EntryPoint = "EndDocPrinter", SetLastError = true)]
    private static extern bool EndDocPrinter(IntPtr hPrinter);

    [DllImport("winspool.Drv", EntryPoint = "StartPagePrinter", SetLastError = true)]
    private static extern bool StartPagePrinter(IntPtr hPrinter);

    [DllImport("winspool.Drv", EntryPoint = "EndPagePrinter", SetLastError = true)]
    private static extern bool EndPagePrinter(IntPtr hPrinter);

    [DllImport("winspool.Drv", EntryPoint = "WritePrinter", SetLastError = true)]
    private static extern bool WritePrinter(IntPtr hPrinter, IntPtr pBytes, int dwCount, out int dwWritten);

    [DllImport("winspool.drv", CharSet = CharSet.Auto, SetLastError = true)]
    private static extern bool EnumPrinters(uint Flags, string? Name, uint Level, IntPtr pPrinterEnum, uint cbBuf, out uint pcbNeeded, out uint pcReturned);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    private class DOCINFO
    {
        [MarshalAs(UnmanagedType.LPTStr)]
        public string? pDocName;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string? pOutputFile;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string? pDatatype;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    private struct PRINTER_INFO_2
    {
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pServerName;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pPrinterName;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pShareName;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pPortName;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pDriverName;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pComment;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pLocation;
        public IntPtr pDevMode;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pSepFile;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pPrintProcessor;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pDatatype;
        [MarshalAs(UnmanagedType.LPTStr)]
        public string pParameters;
        public IntPtr pSecurityDescriptor;
        public uint Attributes;
        public uint Priority;
        public uint DefaultPriority;
        public uint StartTime;
        public uint UntilTime;
        public uint Status;
        public uint cJobs;
        public uint AveragePPM;
    }

    private const uint PRINTER_ENUM_LOCAL = 2;
    private const uint PRINTER_ENUM_CONNECTIONS = 4;

    public async Task<SpoolerResult> PrintAsync(string printerName, byte[] data, string documentName, CancellationToken ct)
    {
        try
        {
            bool success = await Task.Run(() => PrintRawData(printerName, data, documentName), ct);

            if (success)
            {
                return SpoolerResult.Ok(0);
            }
            else
            {
                return SpoolerResult.Fail("Spooler rejected the job");
            }
        }
        catch (Exception ex)
        {
            return SpoolerResult.Fail($"Print failed: {ex.Message}");
        }
    }

    public Task<IReadOnlyList<PrinterInfo>> GetPrintersAsync(CancellationToken ct)
    {
        try
        {
            var printers = GetLocalPrinters();
            return Task.FromResult<IReadOnlyList<PrinterInfo>>(printers);
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[IPP] Failed to get printers: {ex.Message}");
            return Task.FromResult<IReadOnlyList<PrinterInfo>>(Array.Empty<PrinterInfo>());
        }
    }

    private bool PrintRawData(string printerName, byte[] data, string documentName)
    {
        IntPtr hPrinter = IntPtr.Zero;
        IntPtr pBytes = IntPtr.Zero;

        try
        {
            if (!OpenPrinter(printerName.Normalize(), out hPrinter, IntPtr.Zero))
            {
                AppLogger.Error($"[IPP] OpenPrinter failed for '{printerName}'. Error: {Marshal.GetLastWin32Error()}");
                return false;
            }

            var di = new DOCINFO
            {
                pDocName = documentName,
                pDatatype = "RAW"
            };

            int jobId = StartDocPrinter(hPrinter, 1, di);
            if (jobId <= 0)
            {
                AppLogger.Error($"[IPP] StartDocPrinter failed. Error: {Marshal.GetLastWin32Error()}");
                return false;
            }

            if (!StartPagePrinter(hPrinter))
            {
                AppLogger.Error($"[IPP] StartPagePrinter failed. Error: {Marshal.GetLastWin32Error()}");
                return false;
            }

            pBytes = Marshal.AllocCoTaskMem(data.Length);
            Marshal.Copy(data, 0, pBytes, data.Length);

            int dwWritten = 0;
            bool success = WritePrinter(hPrinter, pBytes, data.Length, out dwWritten);

            EndPagePrinter(hPrinter);
            EndDocPrinter(hPrinter);

            if (!success)
            {
                AppLogger.Error($"[IPP] WritePrinter failed. Error: {Marshal.GetLastWin32Error()}");
                return false;
            }

            AppLogger.Log($"[IPP] Printed {dwWritten} bytes to '{printerName}'");
            return true;
        }
        finally
        {
            if (pBytes != IntPtr.Zero)
                Marshal.FreeCoTaskMem(pBytes);
            if (hPrinter != IntPtr.Zero)
                ClosePrinter(hPrinter);
        }
    }

    private List<PrinterInfo> GetLocalPrinters()
    {
        var printers = new List<PrinterInfo>();
        uint flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        uint cbNeeded = 0;
        uint cReturned = 0;

        EnumPrinters(flags, null, 2, IntPtr.Zero, 0, out cbNeeded, out cReturned);

        if (cbNeeded > 0)
        {
            IntPtr pAddr = Marshal.AllocHGlobal((int)cbNeeded);
            try
            {
                if (EnumPrinters(flags, null, 2, pAddr, cbNeeded, out cbNeeded, out cReturned))
                {
                    var infoArray = new PRINTER_INFO_2[cReturned];
                    Type type = typeof(PRINTER_INFO_2);
                    int increment = Marshal.SizeOf(type);

                    for (int i = 0; i < cReturned; i++)
                    {
                        IntPtr currentAddr = IntPtr.Add(pAddr, i * increment);
                        infoArray[i] = (PRINTER_INFO_2)Marshal.PtrToStructure(currentAddr, type)!;
                        printers.Add(new PrinterInfo
                        {
                            Name = infoArray[i].pPrinterName ?? "",
                            DriverName = infoArray[i].pDriverName ?? "",
                            IsOnline = true
                        });
                    }
                }
            }
            finally
            {
                Marshal.FreeHGlobal(pAddr);
            }
        }

        return printers;
    }
}
