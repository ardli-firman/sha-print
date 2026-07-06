using System;
using System.Collections.Generic;
using System.Linq;
using System.Printing;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.Models;
using ShaPrint.WpfApp.ViewModels.Pages;
using ShaPrint.Server;

namespace ShaPrint.WpfApp.Services.Server
{
    public class ServerStatusProvider
    {
        private readonly ServerViewModel _serverViewModel;

        public ServerStatusProvider(ServerViewModel serverViewModel)
        {
            _serverViewModel = serverViewModel;
        }

        public ServerStatusPayload BuildStatus()
        {
            var payload = new ServerStatusPayload
            {
                ServerName = Environment.MachineName,
                HostName = Environment.MachineName,
                NetworkChannel = AppSettings.Current.NetworkChannel,
                Version = typeof(ServerStatusProvider).Assembly.GetName().Version?.ToString() ?? "1.0.0.0",
                UptimeSeconds = GetUptimeSeconds()
            };

            // 1. Gather Printer status
            payload.Printers = GetPrinterStatuses();

            // 2. Gather Scanner status
            payload.Scanners = GetScannerStatuses();

            // 3. Gather Active Clients
            payload.ActiveClients = GetActiveClients();

            // 4. Gather Recent Jobs
            payload.RecentJobs = _serverViewModel.RecentJobs.ToList();

            // 5. Gather Errors
            payload.Errors = _serverViewModel.Errors.ToList();

            return payload;
        }

        private long GetUptimeSeconds()
        {
            if (_serverViewModel.ServerStartTime.HasValue)
            {
                var diff = DateTime.UtcNow - _serverViewModel.ServerStartTime.Value;
                return (long)diff.TotalSeconds;
            }
            return 0;
        }

        private List<PrinterStatus> GetPrinterStatuses()
        {
            var printerStatuses = new List<PrinterStatus>();
            try
            {
                using (var printServer = new LocalPrintServer())
                {
                    foreach (var printerName in _serverViewModel.ExposedPrinters)
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
                        catch (Exception ex)
                        {
                            printerStatuses.Add(new PrinterStatus
                            {
                                Name = printerName,
                                Status = "error",
                                QueueLength = 0,
                                ErrorDescription = $"Offline or unreachable: {ex.Message}"
                            });
                        }
                    }
                }
            }
            catch (Exception ex)
            {
                // Failed to query print server
                foreach (var printerName in _serverViewModel.ExposedPrinters)
                {
                    printerStatuses.Add(new PrinterStatus
                    {
                        Name = printerName,
                        Status = "error",
                        QueueLength = 0,
                        ErrorDescription = $"Print Server query error: {ex.Message}"
                    });
                }
            }
            return printerStatuses;
        }

        private List<ScannerStatus> GetScannerStatuses()
        {
            var scannerStatuses = new List<ScannerStatus>();
            foreach (var scannerName in _serverViewModel.ExposedScanners)
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

        private List<ActiveClientInfo> GetActiveClients()
        {
            var activeClients = new List<ActiveClientInfo>();
            if (_serverViewModel.DiscoveryServer != null)
            {
                var clients = _serverViewModel.DiscoveryServer.GetActiveClientsWithConnectionTimes();
                foreach (var kvp in clients)
                {
                    activeClients.Add(new ActiveClientInfo
                    {
                        Ip = kvp.Key,
                        ConnectedSince = kvp.Value
                    });
                }
            }
            return activeClients;
        }

        private string FormatElapsedTime(DateTime utcTime)
        {
            var span = DateTime.UtcNow - utcTime;
            if (span.TotalSeconds < 60) return $"{(int)span.TotalSeconds}s";
            if (span.TotalMinutes < 60) return $"{(int)span.TotalMinutes}m";
            if (span.TotalHours < 24) return $"{(int)span.TotalHours}h";
            return $"{span.Days}d";
        }
    }
}
