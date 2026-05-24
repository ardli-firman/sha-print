using System.Collections.Generic;

namespace ShaPrint.Core.Network
{
    public class DiscoveryResponseMessage
    {
        public string ServerName { get; set; } = string.Empty;
        public string IpAddress { get; set; } = string.Empty;
        public List<PrinterInfo> ExposedPrinters { get; set; } = new List<PrinterInfo>();

        /// <summary>
        /// HMAC-SHA256 signature of the JSON (excl. this field).
        /// Client MUST verify this before trusting the response.
        /// </summary>
        public string? HmacSignature { get; set; }
    }
}
