using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
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
        public const int MinDpi = 75;
        public const int MaxDpi = 1200;
        public const int MaxFormatBytes = 16;

        public string TargetScannerName { get; set; } = string.Empty;
        public int Dpi { get; set; } = 150;
        public int ColorMode { get; set; } = 2; // 0 = B&W, 1 = Grayscale, 2 = Color
        public string Format { get; set; } = "JPEG";
        public int Brightness { get; set; } = 0; // -100 to 100
        public int Contrast { get; set; } = 0;   // -100 to 100
 
        public static Task WriteAsync(Stream stream, ScanRequestPayload payload)
            => WriteAsync(stream, payload, CancellationToken.None);

        public static async Task WriteAsync(Stream stream, ScanRequestPayload payload, CancellationToken cancellationToken)
        {
            Validate(payload, nameof(payload));

            // Step 1: serialize the inner payload
            byte[] innerPayload;
            using (var ms = new MemoryStream())
            {
                using (var bw = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
                {
                    bw.Write(payload.TargetScannerName);
                    bw.Write(payload.Dpi);
                    bw.Write(payload.ColorMode);
                    bw.Write(payload.Format);
                    bw.Write(payload.Brightness);
                    bw.Write(payload.Contrast);
                    bw.Flush();
                }
                innerPayload = ms.ToArray();
            }
 
            // Step 2: encrypt with AES-256-GCM
            byte[] encryptedBlob = CryptoHelper.EncryptAesGcm(innerPayload);
 
            // Step 3: send [length][encrypted blob]. Use the async stream API so
            // a disconnected client can cancel a slow write instead of blocking a
            // receiver thread indefinitely.
            byte[] length = BitConverter.GetBytes(encryptedBlob.Length);
            await stream.WriteAsync(length, cancellationToken).ConfigureAwait(false);
            await stream.WriteAsync(encryptedBlob, cancellationToken).ConfigureAwait(false);
            await stream.FlushAsync(cancellationToken).ConfigureAwait(false);
        }
 
        public static Task<ScanRequestPayload> ReadAsync(Stream stream)
            => ReadAsync(stream, CancellationToken.None);

        public static async Task<ScanRequestPayload> ReadAsync(Stream stream, CancellationToken cancellationToken)
        {
            try
            {
                byte[] lengthBuffer = new byte[sizeof(int)];
                await ReadExactlyAsync(stream, lengthBuffer, cancellationToken).ConfigureAwait(false);
                int encryptedLength = BitConverter.ToInt32(lengthBuffer, 0);
                if (encryptedLength < 0)
                    throw new InvalidDataException($"Negative encrypted blob length: {encryptedLength}.");
                if (encryptedLength > 8192) // Scan request payload is small, should never exceed 8KB
                    throw new InvalidDataException($"Encrypted blob exceeds limit: {encryptedLength} bytes.");

                byte[] encryptedBlob = new byte[encryptedLength];
                await ReadExactlyAsync(stream, encryptedBlob, cancellationToken).ConfigureAwait(false);
 
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
                using var ms = new MemoryStream(innerPayload, writable: false);
                using var br = new BinaryReader(ms, Encoding.UTF8, leaveOpen: true);
 
                var payload = new ScanRequestPayload();
                payload.TargetScannerName = ReadBoundedString(br, Constants.MaxTargetPrinterNameBytes, "TargetScannerName");
 
                payload.Dpi = br.ReadInt32();
                payload.ColorMode = br.ReadInt32();
                payload.Format = ReadBoundedString(br, MaxFormatBytes, "Format");
 
                // Check for backward compatibility
                if (ms.Position < ms.Length)
                {
                    payload.Brightness = br.ReadInt32();
                }
                if (ms.Position < ms.Length)
                {
                    payload.Contrast = br.ReadInt32();
                }

                if (ms.Position != ms.Length)
                    throw new InvalidDataException("Scan request contains trailing malformed data.");

                Validate(payload, nameof(payload));
 
                return payload;
            }
            catch (EndOfStreamException ex)
            {
                throw new InvalidDataException("Truncated scan request payload.", ex);
            }
        }

        public static void Validate(ScanRequestPayload payload, string parameterName = "payload")
        {
            ArgumentNullException.ThrowIfNull(payload, parameterName);

            if (string.IsNullOrWhiteSpace(payload.TargetScannerName))
                throw new ArgumentException("TargetScannerName cannot be empty.", parameterName);
            if (Encoding.UTF8.GetByteCount(payload.TargetScannerName) > Constants.MaxTargetPrinterNameBytes)
                throw new ArgumentException($"TargetScannerName exceeds {Constants.MaxTargetPrinterNameBytes} bytes.", parameterName);
            if (payload.Dpi < MinDpi || payload.Dpi > MaxDpi)
                throw new ArgumentOutOfRangeException(nameof(payload.Dpi), payload.Dpi, $"DPI must be between {MinDpi} and {MaxDpi}.");
            if (payload.ColorMode is < 0 or > 2)
                throw new ArgumentOutOfRangeException(nameof(payload.ColorMode), payload.ColorMode, "ColorMode must be 0 (B&W), 1 (grayscale), or 2 (color).");
            if (string.IsNullOrWhiteSpace(payload.Format) || Encoding.UTF8.GetByteCount(payload.Format) > MaxFormatBytes)
                throw new ArgumentException("Format is invalid or too long.", nameof(payload.Format));
            if (!IsSupportedFormat(payload.Format))
                throw new ArgumentException("Format must be JPEG, PNG, or PDF.", nameof(payload.Format));
            if (payload.Brightness is < -100 or > 100)
                throw new ArgumentOutOfRangeException(nameof(payload.Brightness), payload.Brightness, "Brightness must be between -100 and 100.");
            if (payload.Contrast is < -100 or > 100)
                throw new ArgumentOutOfRangeException(nameof(payload.Contrast), payload.Contrast, "Contrast must be between -100 and 100.");
        }

        public static bool IsSupportedFormat(string format)
            => format.Equals("JPEG", StringComparison.OrdinalIgnoreCase)
               || format.Equals("JPG", StringComparison.OrdinalIgnoreCase)
               || format.Equals("PNG", StringComparison.OrdinalIgnoreCase)
               || format.Equals("PDF", StringComparison.OrdinalIgnoreCase);

        private static string ReadBoundedString(BinaryReader reader, int maxBytes, string fieldName)
        {
            int byteCount = Read7BitEncodedInt(reader);
            if (byteCount < 0 || byteCount > maxBytes)
                throw new InvalidDataException($"{fieldName} exceeds {maxBytes} bytes.");

            byte[] bytes = reader.ReadBytes(byteCount);
            if (bytes.Length != byteCount)
                throw new EndOfStreamException($"Truncated {fieldName}.");

            try
            {
                return new UTF8Encoding(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true).GetString(bytes);
            }
            catch (DecoderFallbackException ex)
            {
                throw new InvalidDataException($"{fieldName} contains invalid UTF-8.", ex);
            }
        }

        private static int Read7BitEncodedInt(BinaryReader reader)
        {
            int value = 0;
            int shift = 0;
            for (int i = 0; i < 5; i++)
            {
                byte current = reader.ReadByte();
                value |= (current & 0x7f) << shift;
                if ((current & 0x80) == 0)
                    return value;
                shift += 7;
            }

            throw new InvalidDataException("Invalid string length encoding.");
        }

        internal static async Task ReadExactlyAsync(Stream stream, byte[] buffer, CancellationToken cancellationToken)
        {
            int offset = 0;
            while (offset < buffer.Length)
            {
                int read = await stream.ReadAsync(buffer.AsMemory(offset), cancellationToken).ConfigureAwait(false);
                if (read == 0)
                    throw new EndOfStreamException($"Truncated scan request payload: expected {buffer.Length}, got {offset}.");
                offset += read;
            }
        }
    }
}
