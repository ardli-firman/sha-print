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
            byte[] wire = Serialize(payload);
            try
            {
                await stream.WriteAsync(wire).ConfigureAwait(false);
                await stream.FlushAsync().ConfigureAwait(false);
            }
            finally
            {
                CryptographicOperations.ZeroMemory(wire);
            }
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

            try
            {
                return ReadEncryptedBlob(encryptedBlob);
            }
            finally
            {
                CryptographicOperations.ZeroMemory(encryptedBlob);
            }
        }

        /// <summary>Creates the legacy [length][encrypted payload] wire bytes.</summary>
        public static byte[] Serialize(PrintJobPayload payload)
        {
            ArgumentNullException.ThrowIfNull(payload);
            if (string.IsNullOrEmpty(payload.TargetPrinterName))
                throw new ArgumentException("TargetPrinterName cannot be empty.");
            if (payload.TargetPrinterName.Length > Constants.MaxTargetPrinterNameBytes)
                throw new ArgumentException($"TargetPrinterName exceeds {Constants.MaxTargetPrinterNameBytes} bytes.");
            if (payload.SpoolData is null || payload.SpoolData.Length > Constants.MaxPrintJobBytes)
                throw new ArgumentException($"SpoolData exceeds {Constants.MaxPrintJobBytes} bytes.");

            byte[] innerPayload;
            using (var ms = new MemoryStream())
            using (var writer = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
            {
                writer.Write(payload.TargetPrinterName);
                writer.Write(payload.DocumentName ?? string.Empty);
                writer.Write(payload.SpoolData.Length);
                writer.Write(payload.SpoolData);
                writer.Flush();
                innerPayload = ms.ToArray();
            }

            try
            {
                byte[] encrypted = CryptoHelper.EncryptAesGcm(innerPayload);
                try
                {
                    byte[] wire = new byte[sizeof(int) + encrypted.Length];
                    BitConverter.GetBytes(encrypted.Length).CopyTo(wire, 0);
                    Buffer.BlockCopy(encrypted, 0, wire, sizeof(int), encrypted.Length);
                    return wire;
                }
                finally
                {
                    CryptographicOperations.ZeroMemory(encrypted);
                }
            }
            finally
            {
                CryptographicOperations.ZeroMemory(innerPayload);
            }
        }

        /// <summary>Reads a fully-buffered legacy wire payload with exact framing.</summary>
        public static PrintJobPayload ReadWire(ReadOnlySpan<byte> wire)
        {
            if (wire.Length < sizeof(int))
                throw new InvalidDataException("Print job payload is truncated before its length.");

            int encryptedLength = BitConverter.ToInt32(wire[..sizeof(int)]);
            if (encryptedLength < 0 || encryptedLength > Constants.MaxPrintJobBytes + 1024)
                throw new InvalidDataException($"Encrypted blob length is invalid: {encryptedLength}.");
            if (wire.Length != sizeof(int) + encryptedLength)
                throw new InvalidDataException($"Print job payload length mismatch: declared {encryptedLength}, actual {wire.Length - sizeof(int)}.");

            byte[] encrypted = wire[sizeof(int)..].ToArray();
            try
            {
                return ReadEncryptedBlob(encrypted);
            }
            finally
            {
                CryptographicOperations.ZeroMemory(encrypted);
            }
        }

        /// <summary>
        /// Decrypts an owned encrypted blob. The caller retains ownership of the
        /// encrypted argument; decrypted plaintext is always cleared here.
        /// </summary>
        public static PrintJobPayload ReadEncryptedBlob(byte[] encryptedBlob)
        {
            ArgumentNullException.ThrowIfNull(encryptedBlob);

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

            try
            {
                // Deserialize inner payload
                using var ms = new MemoryStream(innerPayload);
                using var br = new BinaryReader(ms, Encoding.UTF8, leaveOpen: true);

                var payload = new PrintJobPayload();

                payload.TargetPrinterName = br.ReadString();
                if (payload.TargetPrinterName.Length > Constants.MaxTargetPrinterNameBytes)
                throw new InvalidDataException(
                    $"TargetPrinterName too long: {payload.TargetPrinterName.Length} bytes (max {Constants.MaxTargetPrinterNameBytes}).");

            // Detect format version (legacy/v1 vs new/v2 with DocumentName)
                long posBeforeDetect = ms.Position;
                bool parsedAsNewFormat = false;

                try
                {
                payload.DocumentName = br.ReadString();
                long remainingAfterDocName = ms.Length - ms.Position;
                if (remainingAfterDocName >= 4)
                {
                    int dataLength = br.ReadInt32();
                    if (dataLength >= 0 && dataLength <= Constants.MaxPrintJobBytes && dataLength == remainingAfterDocName - 4)
                    {
                        payload.SpoolData = br.ReadBytes(dataLength);
                        if (payload.DocumentName.Length > 1024)
                        {
                            payload.DocumentName = payload.DocumentName.Substring(0, 1024);
                        }
                        parsedAsNewFormat = true;
                    }
                }
                }
                catch
                {
                // Fallback to legacy parsing below
                }

                if (!parsedAsNewFormat)
                {
                // Reset position to after TargetPrinterName
                ms.Position = posBeforeDetect;
                long remainingAfterReset = ms.Length - ms.Position;
                if (remainingAfterReset >= 4)
                {
                    int dataLength = br.ReadInt32();
                    if (dataLength < 0)
                        throw new InvalidDataException($"Negative spool data length: {dataLength}.");
                    if (dataLength > Constants.MaxPrintJobBytes)
                        throw new InvalidDataException(
                            $"Spool data exceeds limit: {dataLength} bytes (max {Constants.MaxPrintJobBytes}).");

                    payload.DocumentName = string.Empty;
                    payload.SpoolData = br.ReadBytes(dataLength);
                }
                else
                {
                    throw new InvalidDataException("Invalid print job payload format.");
                }
                }

                return payload;
            }
            finally
            {
                CryptographicOperations.ZeroMemory(innerPayload);
            }
        }
    }
}
