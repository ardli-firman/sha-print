// ShaPrint Core - Constants
using System;
using System.IO;
using System.Security.Cryptography;

namespace ShaPrint.Core
{
    public static class Constants
    {
        public const int DiscoveryUdpPort = 9876;
        public const int PrintTcpPort = 9877;
        public const int MonitorTcpPort = 9878;
        public const string DiscoveryRequestMessage = "SHAPRINT_DISCOVER_REQUEST";
        public const string MonitorDiscoveryRequestMessage = "SHAPRINT_MONITOR_DISCOVER_REQUEST";

        public const int PacketTypePrint = 0x00000001;
        public const int PacketTypeScan = 0x00000002;

        // ── Driver package transfer packet types (auto-provisioning) ─────
        public const int PacketTypeDriverPackageRequest  = 0x20;
        public const int PacketTypeDriverPackageChunk    = 0x21;
        public const int PacketTypeDriverPackageComplete = 0x22;
        public const int PacketTypeDriverPackageError    = 0x23;

        public const int MaxPrintJobBytes          = 104_857_600;
        public const int MaxTargetPrinterNameBytes = 512;
        public const int MaxDiscoveryResponseBytes = 8192;
        public const int MaxConcurrentPrintJobs    = 10;

        // ── Driver provisioning constants ────────────────────────────────
        public const int DriverPackageChunkSize        = 65_536;     // 64 KB per chunk
        public const long MaxDriverPackageSize         = 209_715_200; // 200 MB
        public const int DriverPackageTransferTimeoutMs = 300_000;    // 5 minutes
        public const int DriverPackageCacheTtlHours    = 24;          // server cache TTL
        public const long ClientDriverCacheMaxBytes    = 524_288_000; // 500 MB LRU

        // ─────────────────────────────────────────────
        // Network Channel (Dynamic Shared Secret)
        // ─────────────────────────────────────────────

        private static string _networkChannel = "DefaultChannel";
        
        public static string SharedSecret => _networkChannel;

        public static void SetNetworkChannel(string channelName)
        {
            if (!string.IsNullOrWhiteSpace(channelName))
            {
                _networkChannel = channelName;
                CryptoHelper.InvalidateKeys();
            }
        }
    }
}
