using System.Collections.Generic;

namespace ShaPrint.Core.Network
{
    public class DiscoveryResponseMessage
    {
        public string ServerName { get; set; } = string.Empty;
        public string IpAddress { get; set; } = string.Empty;
        public List<PrinterInfo> ExposedPrinters { get; set; } = new List<PrinterInfo>();
    }
}
