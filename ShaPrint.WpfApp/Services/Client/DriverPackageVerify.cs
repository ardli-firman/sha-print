using System;
using System.IO;
using System.Security.Cryptography;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.WpfApp.Services.Client
{
    /// <summary>
    /// Client-side driver package integrity verification.
    /// NON-NEGOTIABLE: Every driver from the server MUST be verified (SHA-256 + size)
    /// against HMAC-authenticated discovery metadata BEFORE installation.
    /// </summary>
    public static class DriverPackageVerify
    {
        /// <summary>
        /// Verifies a driver package file against expected SHA-256 hash and size.
        /// </summary>
        /// <param name="packagePath">Path to the downloaded package file.</param>
        /// <param name="expectedSha256">Expected SHA-256 hash (hex, lowercase) from HMAC-authenticated discovery.</param>
        /// <param name="expectedSize">Expected size in bytes from HMAC-authenticated discovery.</param>
        /// <returns>True if both size and hash match.</returns>
        public static async Task<bool> VerifyPackageAsync(string packagePath, string expectedSha256, long expectedSize)
        {
            if (!DriverPackageIdValidator.IsValid(expectedSha256))
            {
                AppLogger.Log("[DRIVER_VERIFY] Rejected malformed package identifier.");
                return false;
            }

            if (string.IsNullOrEmpty(packagePath) || !File.Exists(packagePath))
            {
                AppLogger.Log("[DRIVER_VERIFY] Package file not found: " + packagePath);
                return false;
            }

            // Step 1: Size check
            var fileInfo = new FileInfo(packagePath);
            if (fileInfo.Length != expectedSize)
            {
                AppLogger.Log($"[DRIVER_VERIFY] Size mismatch: expected {expectedSize}, got {fileInfo.Length}");
                return false;
            }

            // Step 2: SHA-256 hash check
            try
            {
                using var stream = File.OpenRead(packagePath);
                byte[] hashBytes = await SHA256.HashDataAsync(stream);
                string actualHash = Convert.ToHexString(hashBytes).ToLowerInvariant();

                if (!actualHash.Equals(expectedSha256, StringComparison.OrdinalIgnoreCase))
                {
                    AppLogger.Log($"[DRIVER_VERIFY] SHA-256 mismatch: expected {expectedSha256[..16]}..., got {actualHash[..16]}...");
                    return false;
                }

                AppLogger.Log("[DRIVER_VERIFY] Package integrity verified successfully.");
                return true;
            }
            catch (Exception ex)
            {
                AppLogger.Error("[DRIVER_VERIFY] Verification failed with exception", ex);
                return false;
            }
        }

        /// <summary>
        /// Verifies raw package bytes (in-memory) against expected SHA-256 hash and size.
        /// Used when the package was downloaded and kept in memory.
        /// </summary>
        public static bool VerifyBytes(byte[] data, string expectedSha256, long expectedSize)
        {
            if (!DriverPackageIdValidator.IsValid(expectedSha256))
            {
                AppLogger.Log("[DRIVER_VERIFY] Rejected malformed package identifier.");
                return false;
            }

            if (data == null || data.Length == 0)
            {
                AppLogger.Log("[DRIVER_VERIFY] Empty package data.");
                return false;
            }

            if (data.Length != expectedSize)
            {
                AppLogger.Log($"[DRIVER_VERIFY] Size mismatch: expected {expectedSize}, got {data.Length}");
                return false;
            }

            string actualHash = Convert.ToHexString(SHA256.HashData(data)).ToLowerInvariant();
            if (!actualHash.Equals(expectedSha256, StringComparison.OrdinalIgnoreCase))
            {
                AppLogger.Log($"[DRIVER_VERIFY] SHA-256 mismatch: expected {expectedSha256[..16]}..., got {actualHash[..16]}...");
                return false;
            }

            return true;
        }
    }
}
