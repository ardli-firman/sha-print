using System.Runtime.InteropServices;

namespace ShaPrint.Core.Ipp;

internal interface IWindowsSpoolerNative
{
    bool OpenPrinter(string printerName, out IntPtr handle);
    bool ClosePrinter(IntPtr handle);
    int StartDocPrinter(IntPtr handle, string documentName);
    bool EndDocPrinter(IntPtr handle);
    bool StartPagePrinter(IntPtr handle);
    bool EndPagePrinter(IntPtr handle);
    bool WritePrinter(IntPtr handle, IntPtr bytes, int count, out int written);
    bool SetJob(IntPtr handle, int jobId);
}

/// <summary>Production adapter that writes raw data to the Windows spooler.</summary>
public class WindowsSpoolerAdapter : ISpoolerAdapter
{
    internal static readonly TimeSpan DefaultOperationTimeout = TimeSpan.FromSeconds(120);
    private const int MaxPrinterEnumerationBytes = 16 * 1024 * 1024;
    private const uint PrinterEnumLocal = 2;
    private const uint PrinterEnumConnections = 4;

    private readonly IWindowsSpoolerNative _native;
    private readonly TimeSpan _operationTimeout;

    public WindowsSpoolerAdapter()
        : this(new NativeMethods(), DefaultOperationTimeout)
    {
    }

    internal WindowsSpoolerAdapter(IWindowsSpoolerNative native, TimeSpan operationTimeout)
    {
        _native = native ?? throw new ArgumentNullException(nameof(native));
        if (operationTimeout <= TimeSpan.Zero || operationTimeout == Timeout.InfiniteTimeSpan)
            throw new ArgumentOutOfRangeException(nameof(operationTimeout), "Spooler timeout must be positive and finite.");
        _operationTimeout = operationTimeout;
    }

    public async Task<SpoolerResult> PrintAsync(PrintJob job, CancellationToken ct)
    {
        ArgumentNullException.ThrowIfNull(job);
        if (ct.IsCancellationRequested)
            return SpoolerResult.Fail("Spooler print canceled before it started.");

        if (!string.IsNullOrEmpty(job.DocumentFormat))
            AppLogger.Log($"[IPP] Spooling '{job.DocumentName}' to '{job.PrinterName}' as {job.DocumentFormat} ({job.Data.Length} bytes)");

        var state = new SpoolerJobState();
        Task<bool> worker = Task.Run(() => PrintRawData(job.PrinterName, job.Data, job.DocumentName, state), CancellationToken.None);
        try
        {
            bool success = await worker.WaitAsync(_operationTimeout, ct).ConfigureAwait(false);
            return success ? SpoolerResult.Ok(state.JobId) : SpoolerResult.Fail("Windows spooler rejected the print job.");
        }
        catch (TimeoutException)
        {
            RequestAbort(job.PrinterName, state.JobId);
            return SpoolerResult.Fail($"Windows spooler timed out after {_operationTimeout.TotalSeconds:0.#} seconds.");
        }
        catch (OperationCanceledException) when (ct.IsCancellationRequested)
        {
            RequestAbort(job.PrinterName, state.JobId);
            return SpoolerResult.Fail("Spooler print canceled by the caller.");
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[IPP] Spooler operation failed: {ex.Message}");
            return SpoolerResult.Fail("Windows spooler operation failed.");
        }
    }

    public async Task<IReadOnlyList<PrinterInfo>> GetPrintersAsync(CancellationToken ct)
    {
        try
        {
            return await Task.Run(GetLocalPrinters, CancellationToken.None).WaitAsync(_operationTimeout, ct).ConfigureAwait(false);
        }
        catch (OperationCanceledException) { return Array.Empty<PrinterInfo>(); }
        catch (Exception ex)
        {
            AppLogger.Error($"[IPP] Failed to get printers: {ex.Message}");
            return Array.Empty<PrinterInfo>();
        }
    }

    private bool PrintRawData(string printerName, byte[] data, string documentName, SpoolerJobState state)
    {
        IntPtr hPrinter = IntPtr.Zero;
        IntPtr pBytes = IntPtr.Zero;
        bool printerOpened = false;
        bool documentStarted = false;
        bool pageStarted = false;
        bool success = false;
        try
        {
            if (!_native.OpenPrinter(printerName.Normalize(), out hPrinter))
            {
                AppLogger.Error($"[IPP] OpenPrinter failed for '{printerName}'. Error: {Marshal.GetLastWin32Error()}");
                return false;
            }
            printerOpened = true;

            int jobId = _native.StartDocPrinter(hPrinter, documentName);
            if (jobId <= 0)
            {
                AppLogger.Error($"[IPP] StartDocPrinter failed. Error: {Marshal.GetLastWin32Error()}");
                return false;
            }
            documentStarted = true;
            state.SetJobId(jobId);

            if (!_native.StartPagePrinter(hPrinter))
            {
                AppLogger.Error($"[IPP] StartPagePrinter failed. Error: {Marshal.GetLastWin32Error()}");
                return false;
            }
            pageStarted = true;

            pBytes = Marshal.AllocCoTaskMem(data.Length);
            Marshal.Copy(data, 0, pBytes, data.Length);
            bool writeSucceeded = _native.WritePrinter(hPrinter, pBytes, data.Length, out int written);
            success = writeSucceeded && written == data.Length;
            if (!writeSucceeded)
                AppLogger.Error($"[IPP] WritePrinter failed. Error: {Marshal.GetLastWin32Error()}");
            else if (written != data.Length)
                AppLogger.Error($"[IPP] WritePrinter was partial: expected {data.Length} bytes, wrote {written}.");
            else
                AppLogger.Log($"[IPP] Printed {written} bytes to '{printerName}'");
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[IPP] Native spooler operation failed: {ex.Message}");
            success = false;
        }
        finally
        {
            if (pageStarted)
            {
                try
                {
                    if (!_native.EndPagePrinter(hPrinter))
                    {
                        AppLogger.Error($"[IPP] EndPagePrinter failed. Error: {Marshal.GetLastWin32Error()}");
                        success = false;
                    }
                }
                catch (Exception ex) { AppLogger.Error($"[IPP] EndPagePrinter failed: {ex.Message}"); success = false; }
            }
            if (documentStarted)
            {
                try
                {
                    if (!_native.EndDocPrinter(hPrinter))
                    {
                        AppLogger.Error($"[IPP] EndDocPrinter failed. Error: {Marshal.GetLastWin32Error()}");
                        success = false;
                    }
                }
                catch (Exception ex) { AppLogger.Error($"[IPP] EndDocPrinter failed: {ex.Message}"); success = false; }
            }
            if (pBytes != IntPtr.Zero)
            {
                try { Marshal.FreeCoTaskMem(pBytes); }
                catch (Exception ex) { AppLogger.Error($"[IPP] Failed to release print buffer: {ex.Message}"); success = false; }
            }
            if (printerOpened)
            {
                try
                {
                    if (!_native.ClosePrinter(hPrinter))
                    {
                        AppLogger.Error($"[IPP] ClosePrinter failed. Error: {Marshal.GetLastWin32Error()}");
                        success = false;
                    }
                }
                catch (Exception ex) { AppLogger.Error($"[IPP] ClosePrinter failed: {ex.Message}"); success = false; }
            }
        }
        return success;
    }

    private void RequestAbort(string printerName, int jobId)
    {
        if (jobId <= 0) return;
        _ = Task.Run(() =>
        {
            try
            {
                if (_native.OpenPrinter(printerName.Normalize(), out IntPtr abortHandle))
                {
                    try { _native.SetJob(abortHandle, jobId); }
                    finally { _native.ClosePrinter(abortHandle); }
                }
            }
            catch (Exception ex) { AppLogger.Error($"[IPP] Failed to abort timed out spooler job {jobId}: {ex.Message}"); }
        });
    }

    private List<PrinterInfo> GetLocalPrinters()
    {
        var printers = new List<PrinterInfo>();
        uint flags = PrinterEnumLocal | PrinterEnumConnections;
        uint cbNeeded = 0;
        uint cReturned = 0;
        EnumPrinters(flags, null, 2, IntPtr.Zero, 0, out cbNeeded, out cReturned);
        if (cbNeeded == 0 || cbNeeded > MaxPrinterEnumerationBytes) return printers;

        IntPtr pAddr = Marshal.AllocHGlobal((int)cbNeeded);
        try
        {
            if (!EnumPrinters(flags, null, 2, pAddr, cbNeeded, out cbNeeded, out cReturned)) return printers;
            int increment = Marshal.SizeOf<PRINTER_INFO_2>();
            long required = (long)increment * cReturned;
            if (required > MaxPrinterEnumerationBytes || required > cbNeeded) return printers;
            for (int i = 0; i < cReturned; i++)
            {
                IntPtr currentAddr = IntPtr.Add(pAddr, checked(i * increment));
                var info = Marshal.PtrToStructure<PRINTER_INFO_2>(currentAddr);
                printers.Add(new PrinterInfo { Name = info.pPrinterName ?? string.Empty, DriverName = info.pDriverName ?? string.Empty, IsOnline = true });
            }
        }
        finally { Marshal.FreeHGlobal(pAddr); }
        return printers;
    }

    private sealed class SpoolerJobState
    {
        private int _jobId;
        public int JobId => Volatile.Read(ref _jobId);
        public void SetJobId(int jobId) => Volatile.Write(ref _jobId, jobId);
    }

    private sealed class NativeMethods : IWindowsSpoolerNative
    {
        public bool OpenPrinter(string printerName, out IntPtr handle) => WindowsSpoolerAdapter.OpenPrinter(printerName, out handle, IntPtr.Zero);
        public bool ClosePrinter(IntPtr handle) => WindowsSpoolerAdapter.ClosePrinter(handle);
        public int StartDocPrinter(IntPtr handle, string documentName) => WindowsSpoolerAdapter.StartDocPrinter(handle, 1, new DOCINFO { pDocName = documentName, pDatatype = "RAW" });
        public bool EndDocPrinter(IntPtr handle) => WindowsSpoolerAdapter.EndDocPrinter(handle);
        public bool StartPagePrinter(IntPtr handle) => WindowsSpoolerAdapter.StartPagePrinter(handle);
        public bool EndPagePrinter(IntPtr handle) => WindowsSpoolerAdapter.EndPagePrinter(handle);
        public bool WritePrinter(IntPtr handle, IntPtr bytes, int count, out int written) => WindowsSpoolerAdapter.WritePrinter(handle, bytes, count, out written);
        public bool SetJob(IntPtr handle, int jobId) => WindowsSpoolerAdapter.SetJob(handle, jobId, 0, IntPtr.Zero, 5);
    }

    [DllImport("winspool.Drv", EntryPoint = "OpenPrinter", SetLastError = true, CharSet = CharSet.Auto)]
    private static extern bool OpenPrinter(string printerName, out IntPtr handle, IntPtr defaults);
    [DllImport("winspool.Drv", EntryPoint = "ClosePrinter", SetLastError = true)]
    private static extern bool ClosePrinter(IntPtr handle);
    [DllImport("winspool.Drv", EntryPoint = "StartDocPrinter", SetLastError = true, CharSet = CharSet.Auto)]
    private static extern int StartDocPrinter(IntPtr handle, int level, [In, MarshalAs(UnmanagedType.LPStruct)] DOCINFO docInfo);
    [DllImport("winspool.Drv", EntryPoint = "EndDocPrinter", SetLastError = true)]
    private static extern bool EndDocPrinter(IntPtr handle);
    [DllImport("winspool.Drv", EntryPoint = "StartPagePrinter", SetLastError = true)]
    private static extern bool StartPagePrinter(IntPtr handle);
    [DllImport("winspool.Drv", EntryPoint = "EndPagePrinter", SetLastError = true)]
    private static extern bool EndPagePrinter(IntPtr handle);
    [DllImport("winspool.Drv", EntryPoint = "WritePrinter", SetLastError = true)]
    private static extern bool WritePrinter(IntPtr handle, IntPtr bytes, int count, out int written);
    [DllImport("winspool.Drv", EntryPoint = "SetJob", SetLastError = true)]
    private static extern bool SetJob(IntPtr handle, int jobId, int level, IntPtr job, int command);
    [DllImport("winspool.drv", CharSet = CharSet.Auto, SetLastError = true)]
    private static extern bool EnumPrinters(uint flags, string? name, uint level, IntPtr printerEnum, uint cbBuf, out uint pcbNeeded, out uint pcReturned);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    private sealed class DOCINFO
    {
        [MarshalAs(UnmanagedType.LPTStr)] public string? pDocName;
        [MarshalAs(UnmanagedType.LPTStr)] public string? pOutputFile;
        [MarshalAs(UnmanagedType.LPTStr)] public string? pDatatype;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    private struct PRINTER_INFO_2
    {
        [MarshalAs(UnmanagedType.LPTStr)] public string pServerName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pPrinterName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pShareName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pPortName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pDriverName;
        [MarshalAs(UnmanagedType.LPTStr)] public string pComment;
        [MarshalAs(UnmanagedType.LPTStr)] public string pLocation;
        public IntPtr pDevMode;
        [MarshalAs(UnmanagedType.LPTStr)] public string pSepFile;
        [MarshalAs(UnmanagedType.LPTStr)] public string pPrintProcessor;
        [MarshalAs(UnmanagedType.LPTStr)] public string pDatatype;
        [MarshalAs(UnmanagedType.LPTStr)] public string pParameters;
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
}
