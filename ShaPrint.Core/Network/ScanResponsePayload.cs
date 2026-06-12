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
    ///   [Success: bool] [ErrorMessage: BinaryWriter string] [FileBytesLength: int32] [FileBytes: byte[]]
    /// </summary>
    public class ScanResponsePayload
    {
        public bool Success { get; set; }
        public string ErrorMessage { get; set; } = string.Empty;
        public byte[] FileBytes { get; set; } = Array.Empty<byte>();

        public static async Task WriteAsync(Stream stream, ScanResponsePayload payload)
        {
            // Safeguard bounds check
            if (payload.FileBytes.Length > Constants.MaxPrintJobBytes) // reuse MaxPrintJobBytes (100MB) limit
                throw new ArgumentException($"FileBytes exceeds limit: {payload.FileBytes.Length} bytes.");

            await Task.Run(() =>
            {
                // Step 1: serialize the inner payload
                byte[] innerPayload;
                using (var ms = new MemoryStream())
                using (var bw = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
                {
                    bw.Write(payload.Success);
                    bw.Write(payload.ErrorMessage);
                    bw.Write(payload.FileBytes.Length);
                    bw.Write(payload.FileBytes);
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

        public static async Task<ScanResponsePayload> ReadAsync(Stream stream)
        {
            return await Task.Run(() =>
            {
                using var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);

                int encryptedLength = reader.ReadInt32();
                if (encryptedLength < 0)
                    throw new InvalidDataException($"Negative encrypted blob length: {encryptedLength}.");
                if (encryptedLength > Constants.MaxPrintJobBytes + 1024) // allow overhead
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

                var payload = new ScanResponsePayload();
                payload.Success = br.ReadBoolean();
                payload.ErrorMessage = br.ReadString();

                int fileLength = br.ReadInt32();
                if (fileLength < 0)
                    throw new InvalidDataException($"Negative scan file length: {fileLength}.");
                if (fileLength > Constants.MaxPrintJobBytes)
                    throw new InvalidDataException($"Scan file exceeds limit: {fileLength} bytes.");

                payload.FileBytes = br.ReadBytes(fileLength);
                return payload;
            });
        }
    }
}
