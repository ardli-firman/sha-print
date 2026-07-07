using System;
using System.Collections.Generic;

namespace ShaPrint.Core.Network
{
    public class ServerStatusPayload
    {
        public string ServerName { get; set; } = string.Empty;
        public string HostName { get; set; } = string.Empty;
        public string NetworkChannel { get; set; } = string.Empty;
        public string Version { get; set; } = string.Empty;
        public long UptimeSeconds { get; set; }
        public List<PrinterStatus> Printers { get; set; } = new List<PrinterStatus>();
        public List<ScannerStatus> Scanners { get; set; } = new List<ScannerStatus>();
        public List<ActiveClientInfo> ActiveClients { get; set; } = new List<ActiveClientInfo>();
        public List<JobHistoryEntry> RecentJobs { get; set; } = new List<JobHistoryEntry>();
        public List<ServerErrorEntry> Errors { get; set; } = new List<ServerErrorEntry>();
    }

    public class PrinterStatus
    {
        public string Name { get; set; } = string.Empty;
        public string Status { get; set; } = string.Empty; // e.g. "online", "error", "idle"
        public int QueueLength { get; set; }
        public string? ErrorDescription { get; set; }
    }

    public class ScannerStatus
    {
        public string Name { get; set; } = string.Empty;
        public string Status { get; set; } = string.Empty; // e.g. "available", "inUse", "error"
        public string? LastScanAgo { get; set; }
    }

    public class ActiveClientInfo
    {
        public string Ip { get; set; } = string.Empty;
        public DateTime ConnectedSince { get; set; }
    }

    public class JobHistoryEntry
    {
        public string Type { get; set; } = string.Empty; // "print" or "scan"
        public string Document { get; set; } = string.Empty;
        public string PrinterName { get; set; } = string.Empty;
        public string ClientIp { get; set; } = string.Empty;
        public string Status { get; set; } = string.Empty; // "completed", "failed", "printing"
        public DateTime Timestamp { get; set; }
    }

    public class ServerErrorEntry
    {
        public string Source { get; set; } = string.Empty; // e.g. "PrintMonitor", "PrintReceiver"
        public string Message { get; set; } = string.Empty;
        public DateTime Timestamp { get; set; }
    }
}
