#if WINDOWS
using System;
using System.Collections.Generic;
using System.Printing;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Windows;

namespace ShaPrint.UI.Services
{
    /// <summary>
    /// Windows-only <see cref="ServerStatusProvider"/> that reproduces the exact per-printer /
    /// per-scanner status the WpfApp emitted on the 9878 channel:
    /// <list type="bullet">
    /// <item>printers: <c>LocalPrintServer</c> queue status → "idle"/"online"/"error" with the
    /// same error descriptions ("Out of paper", "Paper jam", …) and per-printer try/catch
    /// fallbacks ("Printer unreachable", "Print server query failed");</item>
    /// <item>scanners: <c>ScannerService.ActiveScans</c> / <c>ScannerService.LastScanTimes</c>
    /// statics → "inUse"/"available" + elapsed-time string.</item>
    /// </list>
    /// Compiled only for the net8.0-windows TFM (#if WINDOWS) because <c>System.Printing</c>
    /// lives in the Windows Desktop framework.
    /// </summary>
    public sealed class WindowsServerStatusProvider : ServerStatusProvider
    {
        public WindowsServerStatusProvider(IServerStatusSource source)
            : base(source)
        {
        }

        protected override List<PrinterStatus> BuildPrinterStatuses()
        {
            var printerStatuses = new List<PrinterStatus>();
            try
            {
                using (var printServer = new LocalPrintServer())
                {
                    foreach (var printerName in Source.ExposedPrinters)
                    {
                        try
                        {
                            using (var queue = printServer.GetPrintQueue(printerName))
                            {
                                queue.Refresh();
                                string status = "idle";
                                bool hasPaperJam = queue.QueueStatus.HasFlag(PrintQueueStatus.PaperJam);
                                bool hasOutOfPaper = queue.QueueStatus.HasFlag(PrintQueueStatus.PaperOut) || queue.IsOutOfPaper;
                                bool hasDoorOpen = queue.QueueStatus.HasFlag(PrintQueueStatus.DoorOpen);
                                bool hasOutOfToner = queue.QueueStatus.HasFlag(PrintQueueStatus.NoToner) || queue.QueueStatus.HasFlag(PrintQueueStatus.TonerLow);
                                bool hasError = queue.QueueStatus.HasFlag(PrintQueueStatus.Error) || queue.IsInError;

                                if (hasOutOfPaper || hasPaperJam || hasOutOfToner || hasDoorOpen || hasError)
                                {
                                    status = "error";
                                }
                                else if (queue.IsPrinting || queue.NumberOfJobs > 0)
                                {
                                    status = "online";
                                }

                                string? errorDesc = null;
                                if (hasOutOfPaper) errorDesc = "Out of paper";
                                else if (hasPaperJam) errorDesc = "Paper jam";
                                else if (hasOutOfToner) errorDesc = "Out of toner";
                                else if (hasDoorOpen) errorDesc = "Printer door open";
                                else if (hasError) errorDesc = "General printer error";

                                printerStatuses.Add(new PrinterStatus
                                {
                                    Name = printerName,
                                    Status = status,
                                    QueueLength = (int)queue.NumberOfJobs,
                                    ErrorDescription = errorDesc
                                });
                            }
                        }
                        catch (Exception)
                        {
                            printerStatuses.Add(new PrinterStatus
                            {
                                Name = printerName,
                                Status = "error",
                                QueueLength = 0,
                                ErrorDescription = "Printer unreachable"
                            });
                        }
                    }
                }
            }
            catch (Exception)
            {
                // Failed to query print server
                foreach (var printerName in Source.ExposedPrinters)
                {
                    printerStatuses.Add(new PrinterStatus
                    {
                        Name = printerName,
                        Status = "error",
                        QueueLength = 0,
                        ErrorDescription = "Print server query failed"
                    });
                }
            }
            return printerStatuses;
        }

        protected override List<ScannerStatus> BuildScannerStatuses()
        {
            var scannerStatuses = new List<ScannerStatus>();
            foreach (var scannerName in Source.ExposedScanners)
            {
                string status = "available";
                if (ScannerService.ActiveScans.ContainsKey(scannerName))
                {
                    status = "inUse";
                }

                string? lastScanStr = null;
                if (ScannerService.LastScanTimes.TryGetValue(scannerName, out var lastScanTime))
                {
                    lastScanStr = FormatElapsedTime(lastScanTime);
                }

                scannerStatuses.Add(new ScannerStatus
                {
                    Name = scannerName,
                    Status = status,
                    LastScanAgo = lastScanStr
                });
            }
            return scannerStatuses;
        }

        private static string FormatElapsedTime(DateTime utcTime)
        {
            var span = DateTime.UtcNow - utcTime;
            if (span.TotalSeconds < 60) return $"{(int)span.TotalSeconds}s";
            if (span.TotalMinutes < 60) return $"{(int)span.TotalMinutes}m";
            if (span.TotalHours < 24) return $"{(int)span.TotalHours}h";
            return $"{span.Days}d";
        }
    }
}
#endif