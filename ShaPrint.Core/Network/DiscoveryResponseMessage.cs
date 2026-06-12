using System.Collections.Generic;
using System.Text.Json.Serialization;

namespace ShaPrint.Core.Network
{
    public class DiscoveryResponseMessage
    {
        public string ServerName { get; set; } = string.Empty;
        public string IpAddress { get; set; } = string.Empty;
        public List<PrinterInfo> ExposedPrinters { get; set; } = new List<PrinterInfo>();

        [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
        public List<ScannerInfo>? ExposedScanners { get; set; }

        /// <summary>
        /// HMAC-SHA256 signature of the JSON (excl. this field).
        /// Omitted from signed JSON via <see cref="JsonIgnoreCondition.WhenWritingNull"/>.
        /// Client MUST verify this before trusting the response.
        /// </summary>
        [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
        public string? HmacSignature { get; set; }
    }
}
