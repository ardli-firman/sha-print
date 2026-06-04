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
        public const string DiscoveryRequestMessage = "SHAPRINT_DISCOVER_REQUEST";

        public const int MaxPrintJobBytes          = 104_857_600;
        public const int MaxTargetPrinterNameBytes = 512;
        public const int MaxDiscoveryResponseBytes = 8192;
        public const int MaxConcurrentPrintJobs    = 10;

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
