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
    ///   [Success: bool] [ErrorMessage: BinaryWriter string] [FileBytesLength: int32] [FileBytes: byte[]]
    /// </summary>
    public class ScanResponsePayload
    {
        public const int MaxErrorMessageBytes = 1024;

        public bool Success { get; set; }
        public string ErrorMessage { get; set; } = string.Empty;
        public byte[] FileBytes { get; set; } = Array.Empty<byte>();

        public static Task WriteAsync(Stream stream, ScanResponsePayload payload)
            => WriteAsync(stream, payload, CancellationToken.None);

        public static async Task WriteAsync(Stream stream, ScanResponsePayload payload, CancellationToken cancellationToken)
        {
            ArgumentNullException.ThrowIfNull(payload);
            if (payload.FileBytes is null)
                throw new ArgumentException("FileBytes cannot be null.", nameof(payload));
            if (payload.FileBytes.Length > Constants.MaxPrintJobBytes)
                throw new ArgumentException($"FileBytes exceeds limit: {payload.FileBytes.Length} bytes.");
            string errorMessage = payload.ErrorMessage ?? string.Empty;
            if (Encoding.UTF8.GetByteCount(errorMessage) > MaxErrorMessageBytes)
                throw new ArgumentException($"ErrorMessage exceeds {MaxErrorMessageBytes} bytes.", nameof(payload));

            // Step 1: serialize the inner payload
            byte[] innerPayload;
            using (var ms = new MemoryStream())
            {
                using (var bw = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
                {
                    bw.Write(payload.Success);
                    bw.Write(errorMessage);
                    bw.Write(payload.FileBytes.Length);
                    bw.Write(payload.FileBytes);
                    bw.Flush();
                }
                innerPayload = ms.ToArray();
            }

            // Step 2: encrypt with AES-256-GCM
            byte[] encryptedBlob = CryptoHelper.EncryptAesGcm(innerPayload);

            // Step 3: send [length][encrypted blob]
            byte[] length = BitConverter.GetBytes(encryptedBlob.Length);
            await stream.WriteAsync(length, cancellationToken).ConfigureAwait(false);
            await stream.WriteAsync(encryptedBlob, cancellationToken).ConfigureAwait(false);
            await stream.FlushAsync(cancellationToken).ConfigureAwait(false);
        }

        public static Task<ScanResponsePayload> ReadAsync(Stream stream)
            => ReadAsync(stream, CancellationToken.None);

        public static async Task<ScanResponsePayload> ReadAsync(Stream stream, CancellationToken cancellationToken)
        {
            try
            {
                byte[] lengthBuffer = new byte[sizeof(int)];
                await ScanRequestPayload.ReadExactlyAsync(stream, lengthBuffer, cancellationToken).ConfigureAwait(false);
                int encryptedLength = BitConverter.ToInt32(lengthBuffer, 0);
                if (encryptedLength < 0)
                    throw new InvalidDataException($"Negative encrypted blob length: {encryptedLength}.");
                if (encryptedLength > Constants.MaxPrintJobBytes + 1024) // allow overhead
                    throw new InvalidDataException($"Encrypted blob exceeds limit: {encryptedLength} bytes.");

                byte[] encryptedBlob = new byte[encryptedLength];
                await ScanRequestPayload.ReadExactlyAsync(stream, encryptedBlob, cancellationToken).ConfigureAwait(false);

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

                var payload = new ScanResponsePayload();
                payload.Success = br.ReadBoolean();
                payload.ErrorMessage = ReadBoundedString(br, MaxErrorMessageBytes, "ErrorMessage");

                int fileLength = br.ReadInt32();
                if (fileLength < 0)
                    throw new InvalidDataException($"Negative scan file length: {fileLength}.");
                if (fileLength > Constants.MaxPrintJobBytes)
                    throw new InvalidDataException($"Scan file exceeds limit: {fileLength} bytes.");

                payload.FileBytes = br.ReadBytes(fileLength);
                if (payload.FileBytes.Length != fileLength)
                    throw new InvalidDataException($"Truncated scan file: expected {fileLength}, got {payload.FileBytes.Length}.");
                if (ms.Position != ms.Length)
                    throw new InvalidDataException("Scan response contains trailing malformed data.");
                return payload;
            }
            catch (EndOfStreamException ex)
            {
                throw new InvalidDataException("Truncated scan response payload.", ex);
            }
        }

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
    }
}
