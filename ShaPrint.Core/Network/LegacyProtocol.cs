using System;
using System.Buffers.Binary;
using System.IO;
using System.Security.Cryptography;
using System.Text;

namespace ShaPrint.Core.Network;

/// <summary>
/// Version constants for the explicit legacy TCP envelope. Version 1 is written
/// with <see cref="BinaryWriter"/>, so all integer fields are little-endian.
/// </summary>
public static class LegacyProtocolVersion
{
    public const byte Current = 1;
}

/// <summary>
/// New legacy-envelope message types. The numeric values retain the established
/// legacy packet type assignments where they already existed.
/// </summary>
public enum LegacyMessageType : byte
{
    PrintJob = Constants.PacketTypePrint,
    ScanRequest = Constants.PacketTypeScan,
    DriverPackageRequest = Constants.PacketTypeDriverPackageRequest,
    DriverPackageChunk = Constants.PacketTypeDriverPackageChunk,
    DriverPackageComplete = Constants.PacketTypeDriverPackageComplete,
    DriverPackageError = Constants.PacketTypeDriverPackageError,
    Acknowledgement = 0x7f
}

/// <summary>
/// Result status returned to a new legacy client. Clients must treat all values
/// other than <see cref="Accepted"/> as terminal for the submitted operation.
/// </summary>
public enum LegacyAcknowledgementStatus : byte
{
    Accepted = 0,
    InvalidPayload = 1,
    Overloaded = 2,
    TargetUnavailable = 3,
    SpoolerRejected = 4,
    Timeout = 5,
    Canceled = 6,
    ServerError = 7
}

/// <summary>
/// Transport-independent frame for new legacy clients.
/// </summary>
public sealed record LegacyEnvelope(
    byte Version,
    LegacyMessageType MessageType,
    long CorrelationId,
    byte[] Payload);

/// <summary>
/// Raw envelope header parsed before version/type/payload validation. Keeping the
/// correlation ID available lets the server reject malformed correlated frames
/// with an actionable terminal acknowledgement.
/// </summary>
public sealed record LegacyEnvelopeHeader(
    byte Version,
    LegacyMessageType MessageType,
    long CorrelationId,
    int PayloadLength);

/// <summary>
/// An encrypted, terminal acknowledgement for a legacy request.
/// </summary>
public sealed record LegacyAcknowledgement(
    long CorrelationId,
    LegacyAcknowledgementStatus Status,
    string Message);

/// <summary>
/// Serializes the new legacy envelope as little-endian:
/// [magic:uint32][version:byte][type:byte][correlationId:int64][payloadLength:int32][payload].
/// The envelope is deliberately transport-agnostic; stream deadline and exact-read
/// handling belongs to the transport layer.
/// </summary>
public static class LegacyEnvelopeCodec
{
    // "SPRT" when viewed as the little-endian uint32 written by BinaryWriter.
    // "SPRT" when read as an ASCII byte sequence. These are public so the
    // stream transport can explicitly discriminate a new frame from the old
    // int32-length payload without guessing from a numeric size.
    public const uint Magic = 0x54525053;
    public const int HeaderLength = sizeof(uint) + sizeof(byte) + sizeof(byte) + sizeof(long) + sizeof(int);
    public const int MaxPayloadBytes = PrintJobPayload.MaxWireBytes;

    public static byte[] Write(LegacyEnvelope envelope)
    {
        ArgumentNullException.ThrowIfNull(envelope);
        ValidateVersion(envelope.Version);
        ValidateMessageType(envelope.MessageType);
        ArgumentNullException.ThrowIfNull(envelope.Payload);
        ValidatePayloadLength(envelope.Payload.Length);

        using var stream = new MemoryStream(HeaderLength + envelope.Payload.Length);
        using var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
        writer.Write(Magic);
        writer.Write(envelope.Version);
        writer.Write((byte)envelope.MessageType);
        writer.Write(envelope.CorrelationId);
        writer.Write(envelope.Payload.Length);
        writer.Write(envelope.Payload);
        writer.Flush();
        return stream.ToArray();
    }

    public static LegacyEnvelope Read(ReadOnlySpan<byte> frame)
    {
        if (frame.Length < HeaderLength)
            throw new InvalidDataException("Legacy envelope is truncated before its header.");

        if (BinaryPrimitives.ReadUInt32LittleEndian(frame) != Magic)
            throw new InvalidDataException("Legacy envelope magic is invalid.");

        byte version = frame[sizeof(uint)];
        ValidateVersion(version);

        var messageType = (LegacyMessageType)frame[sizeof(uint) + sizeof(byte)];
        ValidateMessageType(messageType);

        int correlationOffset = sizeof(uint) + sizeof(byte) + sizeof(byte);
        long correlationId = BinaryPrimitives.ReadInt64LittleEndian(frame.Slice(correlationOffset, sizeof(long)));
        int payloadLength = BinaryPrimitives.ReadInt32LittleEndian(frame.Slice(correlationOffset + sizeof(long), sizeof(int)));
        ValidatePayloadLength(payloadLength);

        int availablePayloadBytes = frame.Length - HeaderLength;
        if (payloadLength != availablePayloadBytes)
        {
            throw new InvalidDataException(
                $"Legacy envelope payload length mismatch: declared {payloadLength}, actual {availablePayloadBytes}.");
        }

        byte[] payload = frame.Slice(HeaderLength, payloadLength).ToArray();

        return new LegacyEnvelope(version, messageType, correlationId, payload);
    }

    public static async Task<LegacyEnvelope> ReadAsync(Stream stream, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(stream);

        byte[] header = new byte[HeaderLength];
        try
        {
            await ReadExactlyAsync(stream, header, cancellationToken).ConfigureAwait(false);
            return await ReadFromHeaderAsync(stream, header, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(header);
        }
    }

    /// <summary>Reads an envelope after the caller has already consumed the magic prefix.</summary>
    public static async Task<LegacyEnvelope> ReadAfterMagicAsync(Stream stream, ReadOnlyMemory<byte> magicPrefix, CancellationToken cancellationToken)
    {
        if (magicPrefix.Length != sizeof(uint) || BinaryPrimitives.ReadUInt32LittleEndian(magicPrefix.Span) != Magic)
            throw new InvalidDataException("Legacy envelope magic is invalid.");

        byte[] header = new byte[HeaderLength];
        try
        {
            magicPrefix.CopyTo(header);
            await ReadExactlyAsync(stream, header.AsMemory(sizeof(uint)), cancellationToken).ConfigureAwait(false);
            return await ReadFromHeaderAsync(stream, header, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(header);
        }
    }

    public static async Task<LegacyEnvelopeHeader> ReadHeaderAfterMagicAsync(Stream stream, ReadOnlyMemory<byte> magicPrefix, CancellationToken cancellationToken)
    {
        if (magicPrefix.Length != sizeof(uint) || BinaryPrimitives.ReadUInt32LittleEndian(magicPrefix.Span) != Magic)
            throw new InvalidDataException("Legacy envelope magic is invalid.");

        byte[] header = new byte[HeaderLength];
        try
        {
            magicPrefix.CopyTo(header);
            await ReadExactlyAsync(stream, header.AsMemory(sizeof(uint)), cancellationToken).ConfigureAwait(false);
            return ParseHeader(header);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(header);
        }
    }

    public static async Task<LegacyEnvelope> ReadPayloadAsync(Stream stream, LegacyEnvelopeHeader header, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(header);
        ValidateVersion(header.Version);
        ValidateMessageType(header.MessageType);
        ValidatePayloadLength(header.PayloadLength);

        byte[] payload = new byte[header.PayloadLength];
        try
        {
            await ReadExactlyAsync(stream, payload, cancellationToken).ConfigureAwait(false);
            return new LegacyEnvelope(header.Version, header.MessageType, header.CorrelationId, payload);
        }
        catch
        {
            CryptographicOperations.ZeroMemory(payload);
            throw;
        }
    }

    public static LegacyEnvelopeHeader ParseHeader(ReadOnlySpan<byte> header)
    {
        if (header.Length != HeaderLength)
            throw new InvalidDataException("Legacy envelope header is truncated.");
        if (BinaryPrimitives.ReadUInt32LittleEndian(header) != Magic)
            throw new InvalidDataException("Legacy envelope magic is invalid.");

        int correlationOffset = sizeof(uint) + sizeof(byte) + sizeof(byte);
        return new LegacyEnvelopeHeader(
            header[sizeof(uint)],
            (LegacyMessageType)header[sizeof(uint) + sizeof(byte)],
            BinaryPrimitives.ReadInt64LittleEndian(header.Slice(correlationOffset, sizeof(long))),
            BinaryPrimitives.ReadInt32LittleEndian(header.Slice(correlationOffset + sizeof(long), sizeof(int))));
    }

    private static async Task<LegacyEnvelope> ReadFromHeaderAsync(Stream stream, byte[] header, CancellationToken cancellationToken)
    {
        return await ReadPayloadAsync(stream, ParseHeader(header), cancellationToken).ConfigureAwait(false);
    }

    public static async Task WriteAsync(Stream stream, LegacyEnvelope envelope, CancellationToken cancellationToken)
    {
        byte[] frame = Write(envelope);
        try
        {
            await stream.WriteAsync(frame, cancellationToken).ConfigureAwait(false);
            await stream.FlushAsync(cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(frame);
        }
    }

    public static async Task ReadExactlyAsync(Stream stream, Memory<byte> buffer, CancellationToken cancellationToken)
    {
        int offset = 0;
        while (offset < buffer.Length)
        {
            int read = await stream.ReadAsync(buffer[offset..], cancellationToken).ConfigureAwait(false);
            if (read == 0)
                throw new EndOfStreamException($"Truncated frame: expected {buffer.Length} bytes, received {offset}.");
            offset += read;
        }
    }

    private static void ValidateVersion(byte version)
    {
        if (version != LegacyProtocolVersion.Current)
            throw new InvalidDataException($"Unsupported legacy protocol version: {version}.");
    }

    private static void ValidateMessageType(LegacyMessageType messageType)
    {
        if (!Enum.IsDefined(messageType))
            throw new InvalidDataException($"Unsupported legacy message type: {(byte)messageType}.");
    }

    private static void ValidatePayloadLength(int payloadLength)
    {
        if (payloadLength < 0 || payloadLength > MaxPayloadBytes)
        {
            throw new InvalidDataException(
                $"Legacy envelope payload length {payloadLength} is outside the allowed range.");
        }
    }
}

/// <summary>
/// Encrypts acknowledgement data with the existing network-channel AES-GCM key.
/// The inner acknowledgement is little-endian:
/// [version:byte][correlationId:int64][status:byte][message:BinaryWriter string].
/// </summary>
public static class LegacyAcknowledgementCodec
{
    private const int AesGcmMinimumEncryptedBytes = 12 + 16; // nonce + authentication tag
    private const int FixedAcknowledgementBytes = sizeof(byte) + sizeof(long) + sizeof(byte);
    private const int MaxMessageBytes = 512;
    private const int MaxEncryptedAcknowledgementBytes = 2_048;
    private static readonly UTF8Encoding StrictUtf8 = new(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true);

    public static byte[] Write(LegacyAcknowledgement acknowledgement)
    {
        ArgumentNullException.ThrowIfNull(acknowledgement);
        ValidateStatus(acknowledgement.Status);

        string message = SanitizeMessage(acknowledgement.Message);
        using var stream = new MemoryStream();
        using (var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true))
        {
            writer.Write(LegacyProtocolVersion.Current);
            writer.Write(acknowledgement.CorrelationId);
            writer.Write((byte)acknowledgement.Status);
            writer.Write(message);
            writer.Flush();
        }

        byte[] encrypted = CryptoHelper.EncryptAesGcm(stream.ToArray());
        if (encrypted.Length > MaxEncryptedAcknowledgementBytes)
            throw new InvalidDataException("Encrypted acknowledgement exceeds its bounded size.");

        return encrypted;
    }

    public static LegacyAcknowledgement Read(ReadOnlySpan<byte> encryptedAcknowledgement)
    {
        if (encryptedAcknowledgement.Length < AesGcmMinimumEncryptedBytes ||
            encryptedAcknowledgement.Length > MaxEncryptedAcknowledgementBytes)
            throw new InvalidDataException("Encrypted acknowledgement length is invalid.");

        byte[] plaintext = CryptoHelper.DecryptAesGcm(encryptedAcknowledgement.ToArray());
        try
        {
            if (plaintext.Length <= FixedAcknowledgementBytes)
                throw new InvalidDataException("Acknowledgement is truncated.");

            int offset = 0;
            byte version = plaintext[offset++];
            if (version != LegacyProtocolVersion.Current)
                throw new InvalidDataException($"Unsupported acknowledgement version: {version}.");

            long correlationId = BinaryPrimitives.ReadInt64LittleEndian(plaintext.AsSpan(offset, sizeof(long)));
            offset += sizeof(long);
            var status = (LegacyAcknowledgementStatus)plaintext[offset++];
            ValidateStatus(status);

            int messageLength = Read7BitEncodedMessageLength(plaintext, ref offset);
            if (messageLength > MaxMessageBytes)
                throw new InvalidDataException("Acknowledgement message exceeds its bounded size.");

            int remainingBytes = plaintext.Length - offset;
            if (messageLength != remainingBytes)
            {
                throw new InvalidDataException(
                    $"Acknowledgement message length mismatch: declared {messageLength}, actual {remainingBytes}.");
            }

            string message = StrictUtf8.GetString(plaintext, offset, messageLength);
            return new LegacyAcknowledgement(correlationId, status, SanitizeMessage(message));
        }
        catch (DecoderFallbackException ex)
        {
            throw new InvalidDataException("Acknowledgement message is not valid UTF-8.", ex);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(plaintext);
        }
    }

    /// <summary>
    /// Writes an encrypted acknowledgement as [length:int32][ciphertext]. The
    /// length prefix lets the peer use a bounded exact read instead of relying
    /// on connection closure to find a message boundary.
    /// </summary>
    public static async Task WriteFramedAsync(Stream stream, LegacyAcknowledgement acknowledgement, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(stream);
        byte[] encrypted = Write(acknowledgement);
        byte[] header = new byte[sizeof(int)];
        try
        {
            BinaryPrimitives.WriteInt32LittleEndian(header, encrypted.Length);
            await stream.WriteAsync(header, cancellationToken).ConfigureAwait(false);
            await stream.WriteAsync(encrypted, cancellationToken).ConfigureAwait(false);
            await stream.FlushAsync(cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(header);
            CryptographicOperations.ZeroMemory(encrypted);
        }
    }

    public static async Task<LegacyAcknowledgement> ReadFramedAsync(Stream stream, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(stream);
        byte[] header = new byte[sizeof(int)];
        try
        {
            await LegacyEnvelopeCodec.ReadExactlyAsync(stream, header, cancellationToken).ConfigureAwait(false);
            int encryptedLength = BinaryPrimitives.ReadInt32LittleEndian(header);
            if (encryptedLength < AesGcmMinimumEncryptedBytes || encryptedLength > MaxEncryptedAcknowledgementBytes)
                throw new InvalidDataException("Encrypted acknowledgement length is invalid.");

            byte[] encrypted = new byte[encryptedLength];
            try
            {
                await LegacyEnvelopeCodec.ReadExactlyAsync(stream, encrypted, cancellationToken).ConfigureAwait(false);
                return Read(encrypted);
            }
            finally
            {
                CryptographicOperations.ZeroMemory(encrypted);
            }
        }
        finally
        {
            CryptographicOperations.ZeroMemory(header);
        }
    }

    private static void ValidateStatus(LegacyAcknowledgementStatus status)
    {
        if (!Enum.IsDefined(status))
            throw new InvalidDataException($"Unsupported acknowledgement status: {(byte)status}.");
    }

    private static int Read7BitEncodedMessageLength(ReadOnlySpan<byte> plaintext, ref int offset)
    {
        uint value = 0;
        for (int index = 0; index < 5; index++)
        {
            if (offset >= plaintext.Length)
                throw new InvalidDataException("Acknowledgement message length is truncated.");

            byte current = plaintext[offset++];
            if (index == 4 && current > 0x07)
                throw new InvalidDataException("Acknowledgement message length is malformed.");

            value |= (uint)(current & 0x7f) << (index * 7);
            if ((current & 0x80) == 0)
                return (int)value;
        }

        throw new InvalidDataException("Acknowledgement message length is malformed.");
    }

    private static string SanitizeMessage(string? message)
    {
        if (string.IsNullOrEmpty(message))
            return string.Empty;

        var builder = new StringBuilder(message.Length);
        bool previousWhitespace = false;
        foreach (char value in message)
        {
            if (char.IsControl(value))
            {
                if (char.IsWhiteSpace(value) && !previousWhitespace)
                {
                    builder.Append(' ');
                    previousWhitespace = true;
                }

                continue;
            }

            if (char.IsWhiteSpace(value))
            {
                if (!previousWhitespace)
                {
                    builder.Append(' ');
                    previousWhitespace = true;
                }

                continue;
            }

            builder.Append(value);
            previousWhitespace = false;
        }

        string sanitized = builder.ToString().Trim();
        while (Encoding.UTF8.GetByteCount(sanitized) > MaxMessageBytes)
            sanitized = sanitized[..^1];

        return sanitized;
    }
}
