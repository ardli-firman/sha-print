using System;
using System.Text.RegularExpressions;

namespace ShaPrint.Core
{
    /// <summary>
    /// Validates all strings received from the network before they are used
    /// in privileged operations (Win32 API calls, filesystem, etc.).
    /// Rejects any input containing shell metacharacters or control characters.
    /// </summary>
    public static class Validators
    {
        // Safe pattern: alphanumeric, spaces, hyphens, underscores, dots, commas, parentheses (for driver names like "Generic / Text Only")
        // EXPLICITLY BLOCKED: single quote, double quote, semicolon, dollar, backtick, pipe, ampersand, angle brackets, newlines
        private static readonly Regex SafeNameRegex = new Regex(
            @"^[\p{L}\p{N}\s\-_\.\,\/\(\)\#]+$",
            RegexOptions.Compiled | RegexOptions.CultureInvariant);

        private static readonly Regex HasDangerousChars = new Regex(
            @"['"";`$&|<>\\\n\r\t\0]",
            RegexOptions.Compiled | RegexOptions.CultureInvariant);

        public const int MaxPrinterNameLength = 220;
        public const int MaxServerNameLength = 64;
        public const int MaxDriverNameLength = 256;

        /// <summary>
        /// Validates a printer name. Must be safe alphanumeric + limited special chars.
        /// Returns the trimmed name on success, throws on failure.
        /// </summary>
        public static string ValidatePrinterName(string? name)
        {
            if (string.IsNullOrWhiteSpace(name))
                throw new ArgumentException("Printer name cannot be empty.");

            string trimmed = name.Trim();
            if (trimmed.Length > MaxPrinterNameLength)
                throw new ArgumentException($"Printer name exceeds {MaxPrinterNameLength} characters.");

            if (HasDangerousChars.IsMatch(trimmed))
                throw new ArgumentException($"Printer name contains invalid characters: '{trimmed}'");

            if (!SafeNameRegex.IsMatch(trimmed))
                throw new ArgumentException($"Printer name contains unsupported characters: '{trimmed}'");

            return trimmed;
        }

        /// <summary>
        /// Validates a server hostname or NetBIOS name.
        /// </summary>
        public static string ValidateServerName(string? name)
        {
            if (string.IsNullOrWhiteSpace(name))
                throw new ArgumentException("Server name cannot be empty.");

            string trimmed = name.Trim();
            if (trimmed.Length > MaxServerNameLength)
                throw new ArgumentException($"Server name exceeds {MaxServerNameLength} characters.");

            if (HasDangerousChars.IsMatch(trimmed))
                throw new ArgumentException($"Server name contains invalid characters: '{trimmed}'");

            if (!SafeNameRegex.IsMatch(trimmed))
                throw new ArgumentException($"Server name contains unsupported characters: '{trimmed}'");

            return trimmed;
        }

        /// <summary>
        /// Validates a printer driver name.
        /// </summary>
        public static string ValidateDriverName(string? name)
        {
            if (string.IsNullOrWhiteSpace(name))
                throw new ArgumentException("Driver name cannot be empty.");

            string trimmed = name.Trim();
            if (trimmed.Length > MaxDriverNameLength)
                throw new ArgumentException($"Driver name exceeds {MaxDriverNameLength} characters.");

            if (HasDangerousChars.IsMatch(trimmed))
                throw new ArgumentException($"Driver name contains invalid characters: '{trimmed}'");

            if (!SafeNameRegex.IsMatch(trimmed))
                throw new ArgumentException($"Driver name contains unsupported characters: '{trimmed}'");

            return trimmed;
        }

        /// <summary>
        /// Validates an IP address string. Returns the trimmed valid IP.
        /// </summary>
        public static string ValidateIpAddress(string? ip)
        {
            if (string.IsNullOrWhiteSpace(ip))
                throw new ArgumentException("IP address cannot be empty.");

            string trimmed = ip.Trim();
            if (!System.Net.IPAddress.TryParse(trimmed, out _))
                throw new ArgumentException($"Invalid IP address: '{trimmed}'");

            return trimmed;
        }
    }
}
