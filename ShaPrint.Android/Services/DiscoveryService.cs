using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using Android.Content;
using Android.Net.Wifi;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Android.Services;

/// <summary>
/// UDP discovery client (port 9876) for Android. The networking + verification logic mirrors
/// <c>ShaPrint.UI.Services.DiscoveryClientService</c> (Task 4; itself migrated from
/// <c>ShaPrint.WpfApp/Services/Client/DiscoveryClient.cs</c>) — copied into this project
/// because ShaPrint.Android must NOT reference ShaPrint.UI (Task 9 anti-pattern). It is pure
/// networking over <c>ShaPrint.Core</c>, plus the Android-only
/// <see cref="WifiManager.MulticastLock"/> held for the duration of the scan so the radio
/// keeps delivering broadcast/multicast replies.
///
/// Documented deviations from the desktop service:
/// <list type="bullet">
///   <item>No per-interface unicast sweep: on Android that means up to ~1024 socket sends over
///   the radio for a /24 subnet, for little gain. The global 255.255.255.255 broadcast + the
///   per-interface subnet-directed broadcasts (same as desktop) + MulticastLock cover the
///   supported scenarios (AP-isolated WiFi is a known limitation, tracked in the plan's
///   troubleshooting notes).</item>
///   <item>The scan is wrapped in a MulticastLock acquire/release (desktop needs no lock).
///   Lock is managed HERE, in the service — never in the Activity — so scans started from a
///   ViewModel keep working across activity recreation.</item>
/// </list>
/// </summary>
public sealed class DiscoveryService
{
    private readonly Context _appContext;

    public DiscoveryService(Context appContext) => _appContext = appContext;

    /// <summary>
    /// Broadcasts <see cref="Constants.DiscoveryRequestMessage"/> on port 9876 and collects
    /// HMAC-verified <see cref="DiscoveryResponseMessage"/> responses for
    /// <paramref name="timeoutMs"/>. Same verification as DiscoveryClientService: responses
    /// without a valid HMAC signature are rejected (unsigned legacy responses are accepted
    /// with a warning, matching desktop behavior).
    /// </summary>
    public async Task<List<DiscoveryResponseMessage>> DiscoverServersAsync(
        int timeoutMs = 2000,
        string? requestMessage = null)
    {
        var servers = new List<DiscoveryResponseMessage>();
        using var udpClient = new UdpClient();
        udpClient.EnableBroadcast = true;

        // MulticastLock lifecycle: acquire BEFORE any send/receive, release in finally so a
        // cancelled/exceptioned scan can never leak the lock.
        WifiManager.MulticastLock? multicastLock = AcquireMulticastLock();
        try
        {
            string msg = requestMessage ?? Constants.DiscoveryRequestMessage;
            byte[] requestData = Encoding.UTF8.GetBytes(msg);

            // 1. Standard 255.255.255.255 broadcast.
            await udpClient.SendAsync(requestData, requestData.Length,
                new IPEndPoint(IPAddress.Broadcast, Constants.DiscoveryUdpPort));

            // 2. Per-interface subnet-directed broadcasts (mirrors DiscoveryClientService).
            try
            {
                foreach (var ni in NetworkInterface.GetAllNetworkInterfaces().Where(n =>
                             n.OperationalStatus == OperationalStatus.Up &&
                             n.NetworkInterfaceType != NetworkInterfaceType.Loopback))
                {
                    foreach (var uipi in ni.GetIPProperties().UnicastAddresses.Where(a =>
                                 a.Address.AddressFamily == AddressFamily.InterNetwork))
                    {
                        var mask = uipi.IPv4Mask;
                        if (mask != null && !mask.Equals(IPAddress.Any))
                        {
                            byte[] ipBytes = uipi.Address.GetAddressBytes();
                            byte[] maskBytes = mask.GetAddressBytes();

                            byte[] broadcastBytes = new byte[ipBytes.Length];
                            for (int i = 0; i < broadcastBytes.Length; i++)
                            {
                                broadcastBytes[i] = (byte)(ipBytes[i] | (maskBytes[i] ^ 255));
                            }
                            await udpClient.SendAsync(requestData, requestData.Length,
                                new IPEndPoint(new IPAddress(broadcastBytes), Constants.DiscoveryUdpPort));
                        }
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Log($"[DISCOVERY] Error enumerating network interfaces: {ex.Message}");
            }

            // 3. Receive window — identical to DiscoveryClientService: parse, HMAC-verify
            //    (drop tampered), trust the packet source over the JSON field, dedupe by IP.
            var tcs = new TaskCompletionSource<bool>();
            _ = Task.Delay(timeoutMs).ContinueWith(t => tcs.TrySetResult(true));

            _ = Task.Run(async () =>
            {
                try
                {
                    while (!tcs.Task.IsCompleted)
                    {
                        UdpReceiveResult result;
                        try
                        {
                            result = await udpClient.ReceiveAsync();
                        }
                        catch (SocketException se) when (se.SocketErrorCode == SocketError.ConnectionReset)
                        {
                            continue; // Ignore ICMP Port Unreachable
                        }

                        string jsonResponse = Encoding.UTF8.GetString(result.Buffer);

                        // Parse without signature first
                        var response = JsonSerializer.Deserialize<DiscoveryResponseMessage>(jsonResponse);
                        if (response == null)
                            continue;

                        // HMAC verification
                        if (!string.IsNullOrEmpty(response.HmacSignature))
                        {
                            // Reconstruct the JSON that was signed (without HmacSignature)
                            string savedSig = response.HmacSignature;
                            response.HmacSignature = null;
                            string unsignedJson = JsonSerializer.Serialize(response);

                            if (!CryptoHelper.VerifyHmac(Encoding.UTF8.GetBytes(unsignedJson), savedSig))
                            {
                                AppLogger.Log($"[DISCOVERY] HMAC verification failed for response from {result.RemoteEndPoint.Address}. Rejecting.");
                                continue; // Drop unauthenticated response
                            }

                            // Restore signature for completeness
                            response.HmacSignature = savedSig;
                        }
                        else
                        {
                            // Legacy response without HMAC — accept but warn
                            AppLogger.Log($"[DISCOVERY] Warning: received unsigned response from {result.RemoteEndPoint.Address}.");
                        }

                        // Overwrite with the actual reachable IP address from the packet source
                        response.IpAddress = result.RemoteEndPoint.Address.ToString();

                        if (!servers.Any(s => s.IpAddress == response.IpAddress))
                        {
                            servers.Add(response);
                        }
                    }
                }
                catch (ObjectDisposedException) { /* udpClient closed, normal */ }
                catch (SocketException ex) when (ex.SocketErrorCode == SocketError.OperationAborted || ex.ErrorCode == 995) { /* udpClient closed, normal */ }
                catch (Exception ex) { AppLogger.Error("Discovery receive error", ex); }
            });

            await tcs.Task;
        }
        finally
        {
            if (multicastLock != null)
            {
                if (multicastLock.IsHeld)
                {
                    multicastLock.Release();
                }
                multicastLock.Dispose();
            }
            udpClient.Close();
        }

        return servers;
    }

    /// <summary>
    /// Acquires a non-reference-counted multicast lock (tagged <c>shaprint-discovery</c>).
    /// Returns null (with a warning) when WifiManager is unavailable — the scan still runs,
    /// it just may miss multicast-delivered replies.
    /// </summary>
    private WifiManager.MulticastLock? AcquireMulticastLock()
    {
        var wifiManager = _appContext.GetSystemService(Context.WifiService) as WifiManager;
        if (wifiManager == null)
        {
            AppLogger.Log("[DISCOVERY] WifiManager unavailable — continuing without MulticastLock.");
            return null;
        }

        var multicastLock = wifiManager.CreateMulticastLock("shaprint-discovery");
        if (multicastLock == null)
        {
            AppLogger.Log("[DISCOVERY] MulticastLock unavailable — continuing without it.");
            return null;
        }

        multicastLock.SetReferenceCounted(false);
        multicastLock.Acquire();
        AppLogger.Log("[DISCOVERY] MulticastLock acquired.");
        return multicastLock;
    }
}