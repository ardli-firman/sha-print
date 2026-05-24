using System;
using System.Security.Cryptography;
using System.Text;

namespace ShaPrint.WpfApp.Helpers
{
    public static class SecretProtector
    {
        private static readonly byte[] Entropy = Encoding.UTF8.GetBytes("ShaPrint-DPAPI-Entropy");

        public static string Protect(string plaintext)
        {
            if (string.IsNullOrEmpty(plaintext)) return string.Empty;
            byte[] plaintextBytes = Encoding.UTF8.GetBytes(plaintext);
            byte[] encryptedBytes = ProtectedData.Protect(plaintextBytes, Entropy, DataProtectionScope.CurrentUser);
            return Convert.ToBase64String(encryptedBytes);
        }

        public static string Unprotect(string base64Ciphertext)
        {
            if (string.IsNullOrEmpty(base64Ciphertext)) return string.Empty;
            try
            {
                byte[] encryptedBytes = Convert.FromBase64String(base64Ciphertext);
                byte[] plaintextBytes = ProtectedData.Unprotect(encryptedBytes, Entropy, DataProtectionScope.CurrentUser);
                return Encoding.UTF8.GetString(plaintextBytes);
            }
            catch (Exception ex)
            {
                ShaPrint.Core.AppLogger.Error("Failed to decrypt secret: " + ex.Message);
                return string.Empty;
            }
        }
    }
}
