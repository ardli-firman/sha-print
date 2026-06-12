using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;

namespace ShaPrint.Core.Network
{
    /// <summary>
    /// Wire format:
    ///   [encryptedLength: int32 (4 bytes)] [encryptedBlob: AES-GCM(nonce || ciphertext || tag)]
    ///
    /// Inner payload (before encryption):
    ///   [TargetScannerName: BinaryWriter string] [Dpi: int32] [ColorMode: int32] [Format: BinaryWriter string]
    /// </summary>
    public class ScanRequestPayload
    {
        public string TargetScannerName { get; set; } = string.Empty;
        public int Dpi { get; set; } = 150;
        public int ColorMode { get; set; } = 2; // 0 = B&W, 1 = Grayscale, 2 = Color
        public string Format { get; set; } = "JPEG";

        public static async Task WriteAsync(Stream stream, ScanRequestPayload payload)
        {
            if (string.IsNullOrEmpty(payload.TargetScannerName))
                throw new ArgumentException("TargetScannerName cannot be empty.");
            if (payload.TargetScannerName.Length > Constants.MaxTargetPrinterNameBytes) // reuse printer name length limit
                throw new ArgumentException($"TargetScannerName exceeds {Constants.MaxTargetPrinterNameBytes} bytes.");

            await Task.Run(() =>
            {
                // Step 1: serialize the inner payload
                byte[] innerPayload;
                using (var ms = new MemoryStream())
                using (var bw = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
                {
                    bw.Write(payload.TargetScannerName);
                    bw.Write(payload.Dpi);
                    bw.Write(payload.ColorMode);
                    bw.Write(payload.Format);
                    bw.Flush();
                    innerPayload = ms.ToArray();
                }

                // Step 2: encrypt with AES-256-GCM
                byte[] encryptedBlob = CryptoHelper.EncryptAesGcm(innerPayload);

                // Step 3: send [length][encrypted blob]
                var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
                writer.Write(encryptedBlob.Length);
                writer.Write(encryptedBlob);
                writer.Flush();
            });
        }

        public static async Task<ScanRequestPayload> ReadAsync(Stream stream)
        {
            return await Task.Run(() =>
            {
                using var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);

                int encryptedLength = reader.ReadInt32();
                if (encryptedLength < 0)
                    throw new InvalidDataException($"Negative encrypted blob length: {encryptedLength}.");
                if (encryptedLength > 8192) // Scan request payload is small, should never exceed 8KB
                    throw new InvalidDataException($"Encrypted blob exceeds limit: {encryptedLength} bytes.");

                byte[] encryptedBlob = reader.ReadBytes(encryptedLength);
                if (encryptedBlob.Length != encryptedLength)
                    throw new InvalidDataException($"Truncated payload: expected {encryptedLength}, got {encryptedBlob.Length}.");

                // Decrypt with AES-256-GCM
                byte[] innerPayload;
                try
                {
                    innerPayload = CryptoHelper.DecryptAesGcm(encryptedBlob);
                }
                catch (CryptographicException ex)
                {
                    throw new InvalidDataException("AES-GCM authentication failed — payload may have been tampered.", ex);
                }

                // Deserialize inner payload
                using var ms = new MemoryStream(innerPayload);
                using var br = new BinaryReader(ms, Encoding.UTF8, leaveOpen: true);

                var payload = new ScanRequestPayload();
                payload.TargetScannerName = br.ReadString();
                if (payload.TargetScannerName.Length > Constants.MaxTargetPrinterNameBytes)
                    throw new InvalidDataException($"TargetScannerName too long.");

                payload.Dpi = br.ReadInt32();
                payload.ColorMode = br.ReadInt32();
                payload.Format = br.ReadString();

                return payload;
            });
        }
    }
}
