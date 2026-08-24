using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace ShaPrint.Server
{
    internal interface ISpoolerApiNative
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

    public static class SpoolerApi
    {
        private const uint MaxPrinterEnumerationBytes = 16 * 1024 * 1024;
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
        public class DOCINFO
        {
            [MarshalAs(UnmanagedType.LPTStr)]
            public string? pDocName;
            [MarshalAs(UnmanagedType.LPTStr)]
            public string? pOutputFile;
            [MarshalAs(UnmanagedType.LPTStr)]
            public string? pDatatype;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
        public struct PRINTER_INFO_2
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

        [DllImport("winspool.Drv", EntryPoint = "OpenPrinter", SetLastError = true, CharSet = CharSet.Auto, ExactSpelling = false, CallingConvention = CallingConvention.StdCall)]
        public static extern bool OpenPrinter([MarshalAs(UnmanagedType.LPTStr)] string szPrinter, out IntPtr hPrinter, IntPtr pd);

        [DllImport("winspool.Drv", EntryPoint = "ClosePrinter", SetLastError = true, ExactSpelling = true, CallingConvention = CallingConvention.StdCall)]
        public static extern bool ClosePrinter(IntPtr hPrinter);

        [DllImport("winspool.Drv", EntryPoint = "SetJob", SetLastError = true, ExactSpelling = false, CallingConvention = CallingConvention.StdCall)]
        public static extern bool SetJob(IntPtr hPrinter, int JobId, int Level, IntPtr pJob, int Command);

        [DllImport("winspool.Drv", EntryPoint = "StartDocPrinter", SetLastError = true, CharSet = CharSet.Auto, ExactSpelling = false, CallingConvention = CallingConvention.StdCall)]
        public static extern int StartDocPrinter(IntPtr hPrinter, int level, [In, MarshalAs(UnmanagedType.LPStruct)] DOCINFO di);

        [DllImport("winspool.Drv", EntryPoint = "EndDocPrinter", SetLastError = true, ExactSpelling = true, CallingConvention = CallingConvention.StdCall)]
        public static extern bool EndDocPrinter(IntPtr hPrinter);

        [DllImport("winspool.Drv", EntryPoint = "StartPagePrinter", SetLastError = true, ExactSpelling = true, CallingConvention = CallingConvention.StdCall)]
        public static extern bool StartPagePrinter(IntPtr hPrinter);

        [DllImport("winspool.Drv", EntryPoint = "EndPagePrinter", SetLastError = true, ExactSpelling = true, CallingConvention = CallingConvention.StdCall)]
        public static extern bool EndPagePrinter(IntPtr hPrinter);

        [DllImport("winspool.Drv", EntryPoint = "WritePrinter", SetLastError = true, ExactSpelling = true, CallingConvention = CallingConvention.StdCall)]
        public static extern bool WritePrinter(IntPtr hPrinter, IntPtr pBytes, int dwCount, out int dwWritten);

        [DllImport("winspool.drv", CharSet = CharSet.Auto, SetLastError = true)]
        public static extern bool EnumPrinters(uint Flags, string? Name, uint Level, IntPtr pPrinterEnum, uint cbBuf, out uint pcbNeeded, out uint pcReturned);

        public const uint PRINTER_ENUM_LOCAL = 2;
        public const uint PRINTER_ENUM_CONNECTIONS = 4;

        public static List<string> GetLocalPrinters()
        {
            try
            {
                var printers = new List<string>();
            uint flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
            uint cbNeeded = 0;
            uint cReturned = 0;

            EnumPrinters(flags, null, 2, IntPtr.Zero, 0, out cbNeeded, out cReturned);
            if (cbNeeded > 0 && cbNeeded <= MaxPrinterEnumerationBytes)
            {
                IntPtr pAddr = Marshal.AllocHGlobal((int)cbNeeded);
                try
                {
                    if (EnumPrinters(flags, null, 2, pAddr, cbNeeded, out cbNeeded, out cReturned))
                    {
                        Type type = typeof(PRINTER_INFO_2);
                        int increment = Marshal.SizeOf(type);
                        if ((long)increment * cReturned > MaxPrinterEnumerationBytes || (long)increment * cReturned > cbNeeded)
                            return printers;

                        for (int i = 0; i < cReturned; i++)
                        {
                            IntPtr currentAddr = IntPtr.Add(pAddr, i * increment);
                            var info = (PRINTER_INFO_2)Marshal.PtrToStructure(currentAddr, type)!;
                            printers.Add(info.pPrinterName);
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
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error($"[SPOOLER] Printer enumeration failed: {ex.Message}");
                return new List<string>();
            }
        }

        public static List<ShaPrint.Core.Network.PrinterInfo> GetLocalPrintersDetailed()
        {
            try
            {
                var printers = new List<ShaPrint.Core.Network.PrinterInfo>();
            uint flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
            uint cbNeeded = 0;
            uint cReturned = 0;

            EnumPrinters(flags, null, 2, IntPtr.Zero, 0, out cbNeeded, out cReturned);
            if (cbNeeded > 0 && cbNeeded <= MaxPrinterEnumerationBytes)
            {
                IntPtr pAddr = Marshal.AllocHGlobal((int)cbNeeded);
                try
                {
                    if (EnumPrinters(flags, null, 2, pAddr, cbNeeded, out cbNeeded, out cReturned))
                    {
                        Type type = typeof(PRINTER_INFO_2);
                        int increment = Marshal.SizeOf(type);
                        if ((long)increment * cReturned > MaxPrinterEnumerationBytes || (long)increment * cReturned > cbNeeded)
                            return printers;

                        for (int i = 0; i < cReturned; i++)
                        {
                            IntPtr currentAddr = IntPtr.Add(pAddr, i * increment);
                            var info = (PRINTER_INFO_2)Marshal.PtrToStructure(currentAddr, type)!;
                            printers.Add(new ShaPrint.Core.Network.PrinterInfo 
                            {
                                Name = info.pPrinterName ?? "",
                                DriverName = info.pDriverName ?? ""
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
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error($"[SPOOLER] Detailed printer enumeration failed: {ex.Message}");
                return new List<ShaPrint.Core.Network.PrinterInfo>();
            }
        }

        public static System.Threading.Tasks.Task<bool> PrintRawDataAsync(
            string printerName,
            byte[] data,
            string documentName,
            TimeSpan? timeout = null,
            System.Threading.CancellationToken cancellationToken = default)
            => PrintRawDataAsync(printerName, data, documentName, timeout, cancellationToken, new Win32SpoolerApiNative());

        internal static async System.Threading.Tasks.Task<bool> PrintRawDataAsync(
            string printerName,
            byte[] data,
            string documentName,
            TimeSpan? timeout,
            System.Threading.CancellationToken cancellationToken,
            ISpoolerApiNative native)
        {
            TimeSpan actualTimeout = timeout ?? TimeSpan.FromSeconds(120);
            if (actualTimeout <= TimeSpan.Zero || actualTimeout == System.Threading.Timeout.InfiniteTimeSpan)
                return false;
            if (cancellationToken.IsCancellationRequested)
                return false;

            var state = new SpoolerJobState();
            var printTask = System.Threading.Tasks.Task.Factory.StartNew(() =>
            {
                IntPtr hPrinter = IntPtr.Zero;
                IntPtr pBytes = IntPtr.Zero;
                bool printerOpened = false;
                bool documentStarted = false;
                bool pageStarted = false;
                bool success = false;
                try
                {
                    ShaPrint.Core.AppLogger.Log($"[SPOOLER] Attempting to open printer: '{printerName}'");
                    if (native.OpenPrinter(printerName.Normalize(), out hPrinter))
                    {
                        printerOpened = true;
                        ShaPrint.Core.AppLogger.Log($"[SPOOLER] Printer opened successfully. Starting document '{documentName}'");
                        int currentJobId = native.StartDocPrinter(hPrinter, documentName);
                        if (currentJobId > 0)
                        {
                            documentStarted = true;
                            state.SetJobId(currentJobId);
                            if (native.StartPagePrinter(hPrinter))
                            {
                                pageStarted = true;
                                pBytes = Marshal.AllocCoTaskMem(data.Length);
                                Marshal.Copy(data, 0, pBytes, data.Length);
                                success = native.WritePrinter(hPrinter, pBytes, data.Length, out int dwWritten)
                                    && dwWritten == data.Length;
                                if (!success && dwWritten != data.Length)
                                    ShaPrint.Core.AppLogger.Error($"[SPOOLER] WritePrinter was partial: expected {data.Length}, wrote {dwWritten}.");
                                else if (!success)
                                    ShaPrint.Core.AppLogger.Error($"[SPOOLER] WritePrinter failed. Win32 Error: {Marshal.GetLastWin32Error()}");
                                else
                                    ShaPrint.Core.AppLogger.Log($"[SPOOLER] WritePrinter wrote {dwWritten} bytes to the spooler.");
                            }
                            else
                            {
                                ShaPrint.Core.AppLogger.Error($"[SPOOLER] StartPagePrinter failed. Win32 Error: {Marshal.GetLastWin32Error()}");
                            }
                        }
                        else
                        {
                            ShaPrint.Core.AppLogger.Error($"[SPOOLER] StartDocPrinter failed. Win32 Error: {Marshal.GetLastWin32Error()}");
                        }
                    }
                    else
                    {
                        ShaPrint.Core.AppLogger.Error($"[SPOOLER] OpenPrinter failed. Win32 Error: {Marshal.GetLastWin32Error()}");
                    }
                }
                catch (Exception ex)
                {
                    ShaPrint.Core.AppLogger.Error($"[SPOOLER] Native spooler operation failed: {ex.Message}");
                    success = false;
                }
                finally
                {
                    if (pageStarted)
                    {
                        try
                        {
                            if (!native.EndPagePrinter(hPrinter))
                                success = false;
                        }
                        catch (Exception ex) { ShaPrint.Core.AppLogger.Error($"[SPOOLER] EndPagePrinter failed: {ex.Message}"); success = false; }
                    }
                    if (documentStarted)
                    {
                        try
                        {
                            if (!native.EndDocPrinter(hPrinter))
                                success = false;
                        }
                        catch (Exception ex) { ShaPrint.Core.AppLogger.Error($"[SPOOLER] EndDocPrinter failed: {ex.Message}"); success = false; }
                    }
                    if (pBytes != IntPtr.Zero)
                    {
                        try { Marshal.FreeCoTaskMem(pBytes); }
                        catch (Exception ex) { ShaPrint.Core.AppLogger.Error($"[SPOOLER] Failed to release print buffer: {ex.Message}"); success = false; }
                    }
                    if (printerOpened)
                    {
                        try
                        {
                            if (!native.ClosePrinter(hPrinter))
                                success = false;
                        }
                        catch (Exception ex) { ShaPrint.Core.AppLogger.Error($"[SPOOLER] ClosePrinter failed: {ex.Message}"); success = false; }
                    }
                }
                return success;
            }, System.Threading.CancellationToken.None, System.Threading.Tasks.TaskCreationOptions.LongRunning, System.Threading.Tasks.TaskScheduler.Default);

            try
            {
                return await printTask.WaitAsync(actualTimeout, cancellationToken);
            }
            catch (TimeoutException)
            {
                ShaPrint.Core.AppLogger.Error($"[SPOOLER] PrintRawData timed out after {actualTimeout.TotalSeconds}s for '{printerName}'.");
                RequestAbort(printerName, state.JobId, native);
                return false;
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                ShaPrint.Core.AppLogger.Error($"[SPOOLER] PrintRawData canceled for '{printerName}'.");
                RequestAbort(printerName, state.JobId, native);
                return false;
            }
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error($"[SPOOLER] PrintRawData failed: {ex.Message}");
                return false;
            }
        }

        private static void RequestAbort(string printerName, int jobId, ISpoolerApiNative native)
        {
            if (jobId <= 0) return;
            _ = System.Threading.Tasks.Task.Run(() =>
            {
                try
                {
                    if (native.OpenPrinter(printerName.Normalize(), out IntPtr abortHandle))
                    {
                        try { native.SetJob(abortHandle, jobId); }
                        finally { native.ClosePrinter(abortHandle); }
                    }
                }
                catch (Exception ex) { ShaPrint.Core.AppLogger.Error($"[SPOOLER] Failed to abort job {jobId}: {ex.Message}"); }
            });
        }

        private sealed class SpoolerJobState
        {
            private int _jobId;
            public int JobId => System.Threading.Volatile.Read(ref _jobId);
            public void SetJobId(int jobId) => System.Threading.Volatile.Write(ref _jobId, jobId);
        }

        private sealed class Win32SpoolerApiNative : ISpoolerApiNative
        {
            public bool OpenPrinter(string printerName, out IntPtr handle) => SpoolerApi.OpenPrinter(printerName, out handle, IntPtr.Zero);
            public bool ClosePrinter(IntPtr handle) => SpoolerApi.ClosePrinter(handle);
            public int StartDocPrinter(IntPtr handle, string documentName) => SpoolerApi.StartDocPrinter(handle, 1, new DOCINFO { pDocName = documentName, pDatatype = "RAW" });
            public bool EndDocPrinter(IntPtr handle) => SpoolerApi.EndDocPrinter(handle);
            public bool StartPagePrinter(IntPtr handle) => SpoolerApi.StartPagePrinter(handle);
            public bool EndPagePrinter(IntPtr handle) => SpoolerApi.EndPagePrinter(handle);
            public bool WritePrinter(IntPtr handle, IntPtr bytes, int count, out int written) => SpoolerApi.WritePrinter(handle, bytes, count, out written);
            public bool SetJob(IntPtr handle, int jobId) => SpoolerApi.SetJob(handle, jobId, 0, IntPtr.Zero, 5);
        }
    }
}
