using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using ShaPrint.Core;

namespace ShaPrint.Core.Network
{
    /// <summary>
    /// Wire format (v2 with AES-256-GCM):
    ///   [encryptedLength: int32 (4 bytes)] [encryptedBlob: AES-GCM(nonce || ciphertext || tag)]
    ///
    /// Inner payload (before encryption):
    ///   [TargetPrinterName: BinaryWriter string] [SpoolData: byte[]]
    /// </summary>
    public class PrintJobPayload
    {
        public string TargetPrinterName { get; set; } = string.Empty;
        public string DocumentName { get; set; } = string.Empty;
        public byte[] SpoolData { get; set; } = Array.Empty<byte>();

        public static async Task WriteAsync(Stream stream, PrintJobPayload payload)
        {
            // Validate before sending
            if (string.IsNullOrEmpty(payload.TargetPrinterName))
                throw new ArgumentException("TargetPrinterName cannot be empty.");
            if (payload.TargetPrinterName.Length > Constants.MaxTargetPrinterNameBytes)
                throw new ArgumentException($"TargetPrinterName exceeds {Constants.MaxTargetPrinterNameBytes} bytes.");
            if (payload.SpoolData.Length > Constants.MaxPrintJobBytes)
                throw new ArgumentException($"SpoolData exceeds {Constants.MaxPrintJobBytes} bytes.");

            await Task.Run(() =>
            {
                // Step 1: serialize the inner payload
                byte[] innerPayload;
                using (var ms = new MemoryStream())
                using (var bw = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
                {
                    bw.Write(payload.TargetPrinterName);
                    bw.Write(payload.DocumentName);
                    bw.Write(payload.SpoolData.Length);
                    bw.Write(payload.SpoolData);
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

        public static async Task<PrintJobPayload> ReadAsync(Stream stream)
        {
            var tcs = new TaskCompletionSource<PrintJobPayload>();
            await Task.Run(() =>
            {
                using var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);
                int encryptedLength = reader.ReadInt32();
                var payload = ReadInternal(reader, encryptedLength);
                tcs.SetResult(payload);
            });
            return await tcs.Task;
        }

        public static PrintJobPayload ReadInternal(BinaryReader reader, int encryptedLength)
        {
            if (encryptedLength < 0)
                throw new InvalidDataException($"Negative encrypted blob length: {encryptedLength}.");
            if (encryptedLength > Constants.MaxPrintJobBytes + 1024) // allow overhead for encryption
                throw new InvalidDataException(
                    $"Encrypted blob exceeds limit: {encryptedLength} bytes (max ~{Constants.MaxPrintJobBytes + 1024}).");

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

            var payload = new PrintJobPayload();

            payload.TargetPrinterName = br.ReadString();
            if (payload.TargetPrinterName.Length > Constants.MaxTargetPrinterNameBytes)
                throw new InvalidDataException(
                    $"TargetPrinterName too long: {payload.TargetPrinterName.Length} bytes (max {Constants.MaxTargetPrinterNameBytes}).");

            payload.DocumentName = br.ReadString();
            if (payload.DocumentName.Length > 1024)
                throw new InvalidDataException("DocumentName too long (max 1024 characters).");

            int dataLength = br.ReadInt32();
            if (dataLength < 0)
                throw new InvalidDataException($"Negative spool data length: {dataLength}.");
            if (dataLength > Constants.MaxPrintJobBytes)
                throw new InvalidDataException(
                    $"Spool data exceeds limit: {dataLength} bytes (max {Constants.MaxPrintJobBytes}).");

            payload.SpoolData = br.ReadBytes(dataLength);
            return payload;
        }
    }
}
