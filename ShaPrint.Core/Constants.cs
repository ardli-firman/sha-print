using System;

namespace ShaPrint.Core
{
    public static class Constants
    {
        public const int DiscoveryUdpPort = 9876;
        public const int PrintTcpPort = 9877;
        public const string DiscoveryRequestMessage = "SHAPRINT_DISCOVER_REQUEST";

        // Security: shared secret for HMAC signing and AES encryption.
        // Override via environment variable SHAPRINT_PSK (min 32 chars).
        public static string SharedSecret
        {
            get
            {
                string? env = Environment.GetEnvironmentVariable("SHAPRINT_PSK");
                if (!string.IsNullOrEmpty(env) && env.Length >= 32)
                    return env;
                return "ShaPrintDefaultPSK_ChangeMeInProduction_2025!";
            }
        }

        // Payload limits to prevent memory exhaustion attacks
        public const int MaxPrintJobBytes          = 104_857_600; // 100 MB
        public const int MaxTargetPrinterNameBytes = 512;
        public const int MaxDiscoveryResponseBytes = 8192;       // 8 KB
        public const int MaxConcurrentPrintJobs    = 10;
    }
}
