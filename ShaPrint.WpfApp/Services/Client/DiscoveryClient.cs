using System;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Client;

public class DiscoveryClient
{
    private const int MaxDiscoveryTargets = 256;
    private const int MaxRequestBytes = 256;
    private static readonly TimeSpan MaximumDiscoveryDeadline = TimeSpan.FromSeconds(30);

    public async Task<List<DiscoveryResponseMessage>> DiscoverServersAsync(
        string? targetIp = null,
        int timeoutMs = 2000,
        bool skipUnicastSweep = false,
        string? requestMessage = null,
        CancellationToken cancellationToken = default)
    {
        if (timeoutMs <= 0)
            throw new ArgumentOutOfRangeException(nameof(timeoutMs));

        var requestedDeadline = TimeSpan.FromMilliseconds(timeoutMs);
        TimeSpan totalDeadline = requestedDeadline <= MaximumDiscoveryDeadline
            ? requestedDeadline
            : MaximumDiscoveryDeadline;

        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        deadline.CancelAfter(totalDeadline);
        CancellationToken token = deadline.Token;

        using var udpClient = new UdpClient { EnableBroadcast = true };
        byte[] requestData = Encoding.UTF8.GetBytes(requestMessage ?? Constants.DiscoveryRequestMessage);
        if (requestData.Length == 0 || requestData.Length > MaxRequestBytes)
        {
            CryptographicOperations.ZeroMemory(requestData);
            throw new ArgumentException("Discovery request is outside the allowed size.", nameof(requestMessage));
        }

        var byAddress = new Dictionary<string, DiscoveryResponseMessage>(StringComparer.OrdinalIgnoreCase);

        try
        {
            IReadOnlyList<IPEndPoint> targets = BuildTargets(targetIp, skipUnicastSweep);
            foreach (var target in targets)
            {
                token.ThrowIfCancellationRequested();
                try
                {
                    await udpClient.SendAsync(requestData, target, token).ConfigureAwait(false);
                }
                catch (SocketException ex)
                {
                    AppLogger.Log($"[DISCOVERY] Send to {target.Address} failed: {ex.SocketErrorCode}.");
                }
            }

            while (true)
            {
                UdpReceiveResult result;
                try
                {
                    result = await udpClient.ReceiveAsync(token).ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
                {
                    break;
                }
                catch (SocketException ex) when (ex.SocketErrorCode == SocketError.ConnectionReset)
                {
                    continue;
                }

                if (result.Buffer.Length == 0 || result.Buffer.Length > Constants.MaxDiscoveryResponseBytes)
                {
                    AppLogger.Log($"[DISCOVERY] Rejected response of {result.Buffer.Length} bytes from {result.RemoteEndPoint.Address}.");
                    continue;
                }

                if (!TryParseResponse(result.Buffer, result.RemoteEndPoint, out var response))
                    continue;

                byAddress.TryAdd(response!.IpAddress, response);
            }
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            // The bounded discovery window elapsed. Return the immutable snapshot collected so far.
        }
        finally
        {
            CryptographicOperations.ZeroMemory(requestData);
        }

        return byAddress.Values.ToList();
    }

    internal static bool TryParseResponse(
        byte[] responseBytes,
        IPEndPoint remoteEndPoint,
        out DiscoveryResponseMessage? response)
    {
        response = null;
        if (responseBytes.Length == 0 || responseBytes.Length > Constants.MaxDiscoveryResponseBytes)
            return false;

        try
        {
            var parsed = JsonSerializer.Deserialize<DiscoveryResponseMessage>(responseBytes);
            if (parsed == null || string.IsNullOrWhiteSpace(parsed.ServerName))
                return false;

            if (!string.IsNullOrEmpty(parsed.HmacSignature))
            {
                string signature = parsed.HmacSignature;
                parsed.HmacSignature = null;
                byte[] unsignedBytes = JsonSerializer.SerializeToUtf8Bytes(parsed);
                try
                {
                    if (!CryptoHelper.VerifyHmac(unsignedBytes, signature))
                    {
                        AppLogger.Log($"[DISCOVERY] HMAC verification failed for {remoteEndPoint.Address}.");
                        return false;
                    }
                }
                finally
                {
                    CryptographicOperations.ZeroMemory(unsignedBytes);
                    parsed.HmacSignature = signature;
                }
            }
            else
            {
                AppLogger.Log($"[DISCOVERY] Warning: received unsigned response from {remoteEndPoint.Address}.");
            }

            parsed.IpAddress = remoteEndPoint.Address.ToString();
            parsed.ExposedPrinters ??= new List<PrinterInfo>();
            response = parsed;
            return true;
        }
        catch (JsonException ex)
        {
            AppLogger.Log($"[DISCOVERY] Protocol error from {remoteEndPoint.Address}: {ex.Message}");
            return false;
        }
    }

    private static IReadOnlyList<IPEndPoint> BuildTargets(string? targetIp, bool skipUnicastSweep)
    {
        if (!string.IsNullOrWhiteSpace(targetIp))
        {
            if (!IPAddress.TryParse(targetIp, out var parsed) || parsed.AddressFamily != AddressFamily.InterNetwork)
                throw new ArgumentException("Discovery target must be a valid IPv4 address.", nameof(targetIp));
            if (!parsed.Equals(IPAddress.Broadcast))
                return new[] { new IPEndPoint(parsed, Constants.DiscoveryUdpPort) };
        }

        var targets = new HashSet<IPAddress> { IPAddress.Broadcast };
        try
        {
            foreach (var networkInterface in NetworkInterface.GetAllNetworkInterfaces()
                .Where(item => item.OperationalStatus == OperationalStatus.Up &&
                               item.NetworkInterfaceType != NetworkInterfaceType.Loopback)
                .Take(16))
            {
                foreach (var unicast in networkInterface.GetIPProperties().UnicastAddresses
                    .Where(item => item.Address.AddressFamily == AddressFamily.InterNetwork))
                {
                    byte[] addressBytes = unicast.Address.GetAddressBytes();
                    byte[]? maskBytes = unicast.IPv4Mask?.GetAddressBytes();
                    if (maskBytes == null || maskBytes.Length != addressBytes.Length)
                        continue;

                    byte[] broadcastBytes = new byte[addressBytes.Length];
                    for (int index = 0; index < broadcastBytes.Length; index++)
                        broadcastBytes[index] = (byte)(addressBytes[index] | (maskBytes[index] ^ 255));

                    targets.Add(new IPAddress(broadcastBytes));
                    if (skipUnicastSweep)
                        continue;

                    uint address = ToUInt32(addressBytes);
                    uint mask = ToUInt32(maskBytes);
                    uint network = address & mask;
                    uint broadcast = network | ~mask;
                    ulong usableHostCount = broadcast > network ? (ulong)broadcast - network - 1 : 0;
                    if (usableHostCount == 0 || usableHostCount > MaxDiscoveryTargets)
                        continue;

                    for (uint candidate = network + 1;
                         candidate < broadcast && targets.Count < MaxDiscoveryTargets;
                         candidate++)
                    {
                        targets.Add(FromUInt32(candidate));
                    }
                }
            }
        }
        catch (Exception ex)
        {
            AppLogger.Log($"[DISCOVERY] Network interface enumeration failed: {ex.GetType().Name}.");
        }

        return targets
            .Take(MaxDiscoveryTargets)
            .Select(address => new IPEndPoint(address, Constants.DiscoveryUdpPort))
            .ToArray();
    }

    private static uint ToUInt32(byte[] bytes)
        => (uint)bytes[0] << 24 | (uint)bytes[1] << 16 | (uint)bytes[2] << 8 | bytes[3];

    private static IPAddress FromUInt32(uint value)
        => new(new[] { (byte)(value >> 24), (byte)(value >> 16), (byte)(value >> 8), (byte)value });
}
