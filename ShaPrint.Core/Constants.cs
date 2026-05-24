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
        // Shared Secret (thread-safe, lazy, never regenerates)
        // Priority:
        //   1. SHAPRINT_PSK  env var              (runtime, cross-machine)
        //   2. BuildTimePsk  embedded at compile   (CI/CD, zero-config deploy)
        //   3. shaprint.key  local file            (dev, survives updates)
        //   4. Auto-generate + persist + warn      (first-run only)
        // ─────────────────────────────────────────────

        private static readonly Lazy<string> _sharedSecret = new(
            ResolveSharedSecret,
            System.Threading.LazyThreadSafetyMode.ExecutionAndPublication);

        public static string SharedSecret => _sharedSecret.Value;

        private static string ResolveSharedSecret()
        {
            // 1. Environment variable (ideal for production — set once per machine)
            string? env = Environment.GetEnvironmentVariable("SHAPRINT_PSK");
            if (!string.IsNullOrEmpty(env) && env.Trim().Length >= 32)
                return env.Trim();

            // 2. Build-time embedded (from GitHub Actions secret → MSBuild property)
            //    Generated file: ShaPrint.Core/BuildTimeSecrets.cs (gitignored)
            string? buildPsk = BuildTimeSecrets.Psk;
            if (!string.IsNullOrEmpty(buildPsk) && buildPsk.Length >= 32)
                return buildPsk;

            // 3. Persisted secret file (set once, survives app reinstall/update)
            string secretFile = GetSecretFilePath();
            if (File.Exists(secretFile))
            {
                try
                {
                    string fileKey = File.ReadAllText(secretFile).Trim();
                    if (fileKey.Length >= 32)
                        return fileKey;
                }
                catch (Exception ex)
                {
                    AppLogger.Error("[SECURITY] Failed to read secret file: " + ex.Message);
                }
            }

            // 4. First run — auto-generate, persist, warn
            string generated = GenerateRandomKey();
            AppLogger.Log(
                "[SECURITY] No shared secret configured!\n" +
                $"A random key has been generated for THIS MACHINE ONLY.\n" +
                "To share printers across machines, set the SAME key everywhere:\n" +
                "  setx SHAPRINT_PSK \"<your-secret-key>\" /M\n\n" +
                $"Key for this machine: {generated}");

            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(secretFile)!);
                File.WriteAllText(secretFile, generated);
            }
            catch (Exception ex)
            {
                AppLogger.Error("[SECURITY] Could not persist auto-generated key: " + ex.Message);
            }

            return generated;
        }

        private static string GetSecretFilePath()
        {
            string dir = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "ShaPrint");
            return Path.Combine(dir, "shaprint.key");
        }

        private static string GenerateRandomKey()
        {
            byte[] bytes = new byte[48];
            RandomNumberGenerator.Fill(bytes);
            return Convert.ToBase64String(bytes);
        }
    }
}
