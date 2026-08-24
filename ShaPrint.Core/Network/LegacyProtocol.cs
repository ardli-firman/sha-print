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
    private const uint Magic = 0x54525053;
    private const int HeaderLength = sizeof(uint) + sizeof(byte) + sizeof(byte) + sizeof(long) + sizeof(int);

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
        if (payloadLength < 0 || payloadLength > Constants.MaxPrintJobBytes)
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
