using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace ShaPrint.Server
{
    public static class SpoolerApi
    {
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
            var printers = new List<string>();
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
                            printers.Add(infoArray[i].pPrinterName);
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

        public static List<ShaPrint.Core.Network.PrinterInfo> GetLocalPrintersDetailed()
        {
            var printers = new List<ShaPrint.Core.Network.PrinterInfo>();
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
                            printers.Add(new ShaPrint.Core.Network.PrinterInfo 
                            {
                                Name = infoArray[i].pPrinterName ?? "",
                                DriverName = infoArray[i].pDriverName ?? ""
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

        public static async System.Threading.Tasks.Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null)
        {
            TimeSpan actualTimeout = timeout ?? TimeSpan.FromSeconds(120);
            int jobId = 0;

            var printTask = System.Threading.Tasks.Task.Factory.StartNew(() =>
            {
                IntPtr pBytes = Marshal.AllocCoTaskMem(data.Length);
                Marshal.Copy(data, 0, pBytes, data.Length);
                bool success = false;
                try
                {
                    IntPtr hPrinter = new IntPtr(0);
                    ShaPrint.Core.AppLogger.Log($"[SPOOLER] Attempting to open printer: '{printerName}'");
                    if (OpenPrinter(printerName.Normalize(), out hPrinter, IntPtr.Zero))
                    {
                        ShaPrint.Core.AppLogger.Log($"[SPOOLER] Printer opened successfully. Starting document '{documentName}'");
                        DOCINFO di = new DOCINFO
                        {
                            pDocName = documentName,
                            pDatatype = "RAW"
                        };

                        int currentJobId = StartDocPrinter(hPrinter, 1, di);
                        if (currentJobId > 0)
                        {
                            System.Threading.Interlocked.Exchange(ref jobId, currentJobId);
                            if (StartPagePrinter(hPrinter))
                            {
                                int dwWritten = 0;
                                success = WritePrinter(hPrinter, pBytes, data.Length, out dwWritten);
                                if (!success)
                                    ShaPrint.Core.AppLogger.Error($"[SPOOLER] WritePrinter failed. Win32 Error: {Marshal.GetLastWin32Error()}");
                                else
                                    ShaPrint.Core.AppLogger.Log($"[SPOOLER] WritePrinter wrote {dwWritten} bytes to the spooler.");

                                EndPagePrinter(hPrinter);
                            }
                            else
                            {
                                ShaPrint.Core.AppLogger.Error($"[SPOOLER] StartPagePrinter failed. Win32 Error: {Marshal.GetLastWin32Error()}");
                            }
                            EndDocPrinter(hPrinter);
                        }
                        else
                        {
                            ShaPrint.Core.AppLogger.Error($"[SPOOLER] StartDocPrinter failed. Win32 Error: {Marshal.GetLastWin32Error()}");
                        }
                        ClosePrinter(hPrinter);
                    }
                    else
                    {
                        ShaPrint.Core.AppLogger.Error($"[SPOOLER] OpenPrinter failed. Win32 Error: {Marshal.GetLastWin32Error()}");
                    }
                }
                finally
                {
                    Marshal.FreeCoTaskMem(pBytes);
                }
                return success;
            }, System.Threading.CancellationToken.None, System.Threading.Tasks.TaskCreationOptions.LongRunning, System.Threading.Tasks.TaskScheduler.Default);

            if (await System.Threading.Tasks.Task.WhenAny(printTask, System.Threading.Tasks.Task.Delay(actualTimeout)) == printTask)
            {
                return await printTask;
            }
            else
            {
                ShaPrint.Core.AppLogger.Error($"[SPOOLER] PrintRawData timed out after {actualTimeout.TotalSeconds}s for '{printerName}'.");
                int capturedJobId = System.Threading.Volatile.Read(ref jobId);
                if (capturedJobId > 0)
                {
                    ShaPrint.Core.AppLogger.Log($"[SPOOLER] Attempting to abort stuck Job ID {capturedJobId}...");
                    IntPtr abortHandle = IntPtr.Zero;
                    if (OpenPrinter(printerName.Normalize(), out abortHandle, IntPtr.Zero))
                    {
                        bool deleted = SetJob(abortHandle, capturedJobId, 0, IntPtr.Zero, 5 /* JOB_CONTROL_DELETE */);
                        if (deleted)
                            ShaPrint.Core.AppLogger.Log($"[SPOOLER] Successfully aborted Job ID {capturedJobId}.");
                        else
                            ShaPrint.Core.AppLogger.Error($"[SPOOLER] Failed to abort Job ID {capturedJobId}. Win32 Error: {Marshal.GetLastWin32Error()}");
                        
                        ClosePrinter(abortHandle);
                    }
                }
                return false;
            }
        }
    }
}
