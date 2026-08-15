using System.Text.Json.Serialization;

namespace ShaPrint.Core.Network
{
    public class PrinterInfo
    {
        public string Name { get; set; } = string.Empty;
        public string Description { get; set; } = string.Empty;
        public string DriverName { get; set; } = string.Empty;

        // ── Driver provisioning metadata (auto-provisioning) ─────────────

        /// <summary>
        /// Whether the server has a driver package available for this printer.
        /// False when DriverSharing is disabled or the package hasn't been cached yet.
        /// </summary>
        public bool DriverAvailable { get; set; }

        /// <summary>
        /// SHA-256 hash of the driver package (hex, lowercase, 64 chars).
        /// Used by the client to verify integrity after download.
        /// Null when DriverAvailable is false.
        /// </summary>
        [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
        public string? DriverPackageId { get; set; }

        /// <summary>
        /// Total size of the driver package in bytes.
        /// 0 when DriverAvailable is false.
        /// </summary>
        public long DriverSizeBytes { get; set; }
    }
}
