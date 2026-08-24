using System;
using System.Buffers.Binary;
using System.IO;
using System.Security.Cryptography;
using System.Threading;
using System.Threading.Tasks;

namespace ShaPrint.Core.Network;

/// <summary>
/// Exact, bounded framing for monitor traffic.
/// </summary>
public static class MonitorFrameCodec
{
    public const int OverloadedFrameLength = -2;

    public static async Task<byte[]> ReadAsync(
        Stream stream,
        int maxPayloadBytes,
        CancellationToken cancellationToken,
        TimeSpan? idleTimeout = null)
    {
        ArgumentNullException.ThrowIfNull(stream);
        if (maxPayloadBytes <= 0)
            throw new ArgumentOutOfRangeException(nameof(maxPayloadBytes));

        byte[] header = new byte[sizeof(int)];
        try
        {
            await ReadExactlyAsync(stream, header, cancellationToken, idleTimeout).ConfigureAwait(false);
            int payloadLength = BinaryPrimitives.ReadInt32LittleEndian(header);
            if (payloadLength == OverloadedFrameLength)
                throw new MonitorOverloadedException();
            if (payloadLength < 0 || payloadLength > maxPayloadBytes)
                throw new InvalidDataException($"Monitor frame length {payloadLength} is outside the allowed range.");

            byte[] payload = new byte[payloadLength];
            try
            {
                await ReadExactlyAsync(stream, payload, cancellationToken, idleTimeout).ConfigureAwait(false);
                return payload;
            }
            catch
            {
                CryptographicOperations.ZeroMemory(payload);
                throw;
            }
        }
        finally
        {
            CryptographicOperations.ZeroMemory(header);
        }
    }

    public static async Task WriteAsync(
        Stream stream,
        ReadOnlyMemory<byte> payload,
        int maxPayloadBytes,
        CancellationToken cancellationToken,
        TimeSpan? idleTimeout = null)
    {
        ArgumentNullException.ThrowIfNull(stream);
        if (payload.Length > maxPayloadBytes)
            throw new InvalidDataException($"Monitor frame length {payload.Length} exceeds {maxPayloadBytes} bytes.");

        byte[] header = new byte[sizeof(int)];
        try
        {
            BinaryPrimitives.WriteInt32LittleEndian(header, payload.Length);
            await WriteWithIdleTimeoutAsync(stream, header, cancellationToken, idleTimeout).ConfigureAwait(false);
            await WriteWithIdleTimeoutAsync(stream, payload, cancellationToken, idleTimeout).ConfigureAwait(false);
            await FlushWithIdleTimeoutAsync(stream, cancellationToken, idleTimeout).ConfigureAwait(false);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(header);
        }
    }

    public static async Task WriteOverloadedAsync(
        Stream stream,
        CancellationToken cancellationToken,
        TimeSpan? idleTimeout = null)
    {
        ArgumentNullException.ThrowIfNull(stream);
        byte[] marker = new byte[sizeof(int)];
        try
        {
            BinaryPrimitives.WriteInt32LittleEndian(marker, OverloadedFrameLength);
            await WriteWithIdleTimeoutAsync(stream, marker, cancellationToken, idleTimeout).ConfigureAwait(false);
            await FlushWithIdleTimeoutAsync(stream, cancellationToken, idleTimeout).ConfigureAwait(false);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(marker);
        }
    }

    private static async Task ReadExactlyAsync(
        Stream stream,
        Memory<byte> buffer,
        CancellationToken cancellationToken,
        TimeSpan? idleTimeout)
    {
        int offset = 0;
        while (offset < buffer.Length)
        {
            int read = await ReadWithIdleTimeoutAsync(
                stream,
                buffer[offset..],
                cancellationToken,
                idleTimeout).ConfigureAwait(false);
            if (read == 0)
                throw new EndOfStreamException($"Truncated monitor frame: expected {buffer.Length} bytes, received {offset}.");
            offset += read;
        }
    }

    private static async Task<int> ReadWithIdleTimeoutAsync(
        Stream stream,
        Memory<byte> buffer,
        CancellationToken cancellationToken,
        TimeSpan? idleTimeout)
    {
        using var idle = CreateIdleToken(cancellationToken, idleTimeout);
        try
        {
            return await stream.ReadAsync(buffer, idle?.Token ?? cancellationToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested && idle?.IsCancellationRequested == true)
        {
            throw new TimeoutException("Monitor stream read exceeded the idle deadline.");
        }
    }

    private static async Task WriteWithIdleTimeoutAsync(
        Stream stream,
        ReadOnlyMemory<byte> buffer,
        CancellationToken cancellationToken,
        TimeSpan? idleTimeout)
    {
        using var idle = CreateIdleToken(cancellationToken, idleTimeout);
        try
        {
            await stream.WriteAsync(buffer, idle?.Token ?? cancellationToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested && idle?.IsCancellationRequested == true)
        {
            throw new TimeoutException("Monitor stream write exceeded the idle deadline.");
        }
    }

    private static async Task FlushWithIdleTimeoutAsync(
        Stream stream,
        CancellationToken cancellationToken,
        TimeSpan? idleTimeout)
    {
        using var idle = CreateIdleToken(cancellationToken, idleTimeout);
        try
        {
            await stream.FlushAsync(idle?.Token ?? cancellationToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested && idle?.IsCancellationRequested == true)
        {
            throw new TimeoutException("Monitor stream flush exceeded the idle deadline.");
        }
    }

    private static CancellationTokenSource? CreateIdleToken(
        CancellationToken cancellationToken,
        TimeSpan? idleTimeout)
    {
        if (idleTimeout is null || idleTimeout <= TimeSpan.Zero)
            return null;

        var idle = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        idle.CancelAfter(idleTimeout.Value);
        return idle;
    }
}

public sealed class MonitorOverloadedException : IOException
{
    public MonitorOverloadedException() : base("Monitor server is overloaded.") { }
}
