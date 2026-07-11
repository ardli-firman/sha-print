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

        /// <summary>
        /// Stable per-server UUID generated once on first start, persisted in ServerConfig.json.
        /// Null for old servers (pre-ServerId). Clients use this as the primary match key.
        /// Omitted from the JSON when null so old clients ignore it.
        /// </summary>
        [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
        public string? ServerId { get; set; }
    }
}
