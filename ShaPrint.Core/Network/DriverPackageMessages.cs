using System.Text.Json.Serialization;

namespace ShaPrint.Core.Network
{
    /// <summary>
    /// Client → Server: request to send a driver package.
    /// </summary>
    public class DriverPackageRequest
    {
        /// <summary>Name of the printer whose driver is requested.</summary>
        public string PrinterName { get; set; } = string.Empty;

        /// <summary>SHA-256 hash (hex) of the expected driver package.</summary>
        public string DriverPackageId { get; set; } = string.Empty;
    }

    /// <summary>
    /// Server → Client: one chunk of the driver package.
    /// </summary>
    public class DriverPackageChunk
    {
        public int ChunkIndex { get; set; }
        public int TotalChunks { get; set; }

        /// <summary>AES-GCM encrypted chunk data (nonce + ciphertext + tag).</summary>
        public byte[] Data { get; set; } = System.Array.Empty<byte>();

        /// <summary>HMAC-SHA256 of the raw (pre-encryption) chunk data.</summary>
        public string ChunkHmac { get; set; } = string.Empty;
    }

    /// <summary>
    /// Server → Client: transfer complete with metadata.
    /// </summary>
    public class DriverPackageComplete
    {
        public long TotalBytes { get; set; }
        public int TotalChunks { get; set; }
    }

    /// <summary>
    /// Server → Client: error during driver package transfer.
    /// </summary>
    public class DriverPackageError
    {
        public string Message { get; set; } = string.Empty;
    }

    /// <summary>
    /// Manifest stored alongside the driver package cache.
    /// </summary>
    public class DriverPackageManifest
    {
        public string InfName { get; set; } = string.Empty;
        public string DriverName { get; set; } = string.Empty;
        public string Sha256 { get; set; } = string.Empty;
        public long TotalSizeBytes { get; set; }
        public int FileCount { get; set; }
        public string ExportedAt { get; set; } = string.Empty;
        public string WindowsVersion { get; set; } = string.Empty;
        public string Architecture { get; set; } = string.Empty;
    }
}
