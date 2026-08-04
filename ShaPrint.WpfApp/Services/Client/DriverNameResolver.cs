using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.RegularExpressions;

namespace ShaPrint.Client
{
    /// <summary>
    /// Resolves the best locally-installed printer driver for a virtual printer install.
    ///
    /// The Server advertises its physical printer's driver name (e.g. "EPSON L120 Series ESC/P-R")
    /// as a *hint*. The Client must resolve that hint against its own locally-installed drivers,
    /// because the exact name may not exist locally (different Epson installer version, regional
    /// variant, "(Copy N)" duplicates, ESC/P vs ESC/P-R suffix differences, etc.).
    ///
    /// Resolution priority (see issue #21):
    ///   1. Exact match (case-insensitive)
    ///   2. Normalized token equality
    ///   3. Model-token match (the hint's distinctive model token must appear in the local driver)
    ///   4. Weighted token scoring as a tiebreaker
    ///   5. null → caller shows the driver picker / confirmation flow
    /// </summary>
    public static class DriverNameResolver
    {
        /// <summary>
        /// Words that do NOT identify a printer model (common across driver names). Used to isolate
        /// the model-bearing tokens so cross-model matches (e.g. L3110 vs L120) are avoided.
        /// </summary>
        private static readonly HashSet<string> GenericTokens = new(StringComparer.Ordinal)
        {
            // ── Vendor names ────────────────────────────────────────────────────────────────
            "epson", "hp", "canon", "brother", "lexmark", "dell", "samsung", "xerox", "ricoh",
            "kyocera", "konica", "minolta", "fuji", "fujixerox", "oki", "panasonic", "sharp",
            "toshiba", "zebra", "datacard", "dymo",
            // ── Product-line / product-family words (not model discriminators) ────────────
            // Epson: inkjet lines
            "stylus", "expression", "artisan", "ecotank",
            // HP: consumer/business lines
            "laserjet", "deskjet", "officejet", "envy", "pagewide", "designjet", "colorlaserjet",
            // Canon: inkjet/laser lines
            "pixma", "imageclass", "imagerunner", "imageprograf", "selphy",
            // Brother: laser/inkjet lines
            "mfc", "dcp", "hl",
            // Lexmark / Xerox / Ricoh: generic product families
            "phaser", "workcentre", "versalink", "altalink", "aficio",
            // ── Common descriptor words ──────────────────────────────────────────────────────
            "series", "driver", "printer", "standard", "universal", "photo", "inkjet", "laser",
            "color", "colour", "black", "white", "plus", "pro", "mini", "max", "basic",
            "advanced", "text", "only", "generic", "network", "wireless", "multifunction",
            "all-in-one", "aio",
        };

        /// <summary>
        /// Normalizes a driver name for comparison: lowercases, collapses whitespace,
        /// and strips common Epson/Windows registration artifacts.
        /// </summary>
        public static string Normalize(string? name)
        {
            if (string.IsNullOrWhiteSpace(name)) return string.Empty;

            string s = name.ToLowerInvariant();

            // Strip parenthesized suffixes like "(Copy 1)", "(Bidirection Support)", "(ESC/P-R)" etc.
            s = Regex.Replace(s, @"\([^)]*\)", " ");

            // Strip trailing driver-mode suffixes that don't change the underlying model.
            s = Regex.Replace(s, @"\besc/p(-r)?\b", " ");

            // Collapse whitespace and trim.
            s = Regex.Replace(s, @"\s+", " ").Trim();

            return s;
        }

        /// <summary>
        /// Resolves the server-advertised driver name against the locally installed drivers.
        /// Returns the best local driver name, or null if nothing matches.
        /// </summary>
        /// <param name="serverDriverName">Driver name advertised by the Server (hint only).</param>
        /// <param name="localDrivers">Names of drivers installed on this machine (e.g. from Get-PrinterDriver).</param>
        public static string? Resolve(string? serverDriverName, IEnumerable<string>? localDrivers)
        {
            var drivers = localDrivers?.Where(d => !string.IsNullOrWhiteSpace(d)).ToList() ?? new List<string>();
            if (drivers.Count == 0) return null;

            string hint = serverDriverName?.Trim() ?? string.Empty;
            if (string.IsNullOrWhiteSpace(hint)) return null;

            // 1. Exact match (case-insensitive, whole-name).
            var exact = drivers.FirstOrDefault(d =>
                string.Equals(d.Trim(), hint, StringComparison.OrdinalIgnoreCase));
            if (exact != null) return exact;

            string hintNorm = Normalize(hint);
            if (string.IsNullOrEmpty(hintNorm)) return null;

            // 2. Normalized exact equality.
            var normExact = drivers.FirstOrDefault(d =>
                string.Equals(Normalize(d), hintNorm, StringComparison.Ordinal));
            if (normExact != null) return normExact;

            // 3+4. Tokenized match with a model-token requirement.
            var hintTokens = hintNorm.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            if (hintTokens.Length == 0) return null;

            // Distinctive (model-bearing) tokens of the hint: exclude generic words.
            var hintModelTokens = hintTokens.Where(t => !GenericTokens.Contains(t)).ToList();

            // The model code/name is usually the non-generic token that CONTAINS A DIGIT
            // (e.g. "l120", "wf-2850", "p1102"). Prefer digit-bearing tokens over the longest
            // token — for multi-token models ("EPSON Expression XP-550") the digit-bearing
            // token ("xp-550") is the reliable discriminator, not the longest word ("expression").
            string? hintModel = hintModelTokens
                .OrderByDescending(t => t.Any(char.IsDigit))
                .ThenByDescending(t => t.Length)
                .FirstOrDefault();

            string? best = null;
            double bestScore = 0.0;

            foreach (var driver in drivers)
            {
                var dNorm = Normalize(driver);
                if (string.IsNullOrEmpty(dNorm)) continue;
                var dTokens = dNorm.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (dTokens.Length == 0) continue;

                // If the hint has a detectable model token, the candidate MUST contain it.
                if (hintModel != null && !dTokens.Contains(hintModel)) continue;

                // Weighted token scoring over shared tokens (favors longer/distinctive tokens).
                int hitCount = 0;
                double hitWeight = 0.0;
                foreach (var dt in dTokens)
                {
                    if (hintTokens.Contains(dt))
                    {
                        hitCount++;
                        hitWeight += dt.Length;
                    }
                }

                if (hitCount == 0) continue;

                int denominator = Math.Min(hintTokens.Length, dTokens.Length);
                if (denominator == 0) continue;
                double coverage = (double)hitCount / denominator;
                double score = coverage * hitWeight;

                if (score > bestScore)
                {
                    bestScore = score;
                    best = driver;
                }
            }

            return best;
        }

        /// <summary>
        /// Returns true if a driver name matches a search filter (substring, case-insensitive).
        /// Empty/whitespace filter matches everything.
        /// </summary>
        public static bool MatchesFilter(string? driverName, string? filter)
        {
            if (string.IsNullOrWhiteSpace(driverName)) return false;
            if (string.IsNullOrWhiteSpace(filter)) return true;
            return driverName.Contains(filter.Trim(), StringComparison.OrdinalIgnoreCase);
        }

        /// <summary>
        /// Returns true if the server-advertised name differs from the resolved local name
        /// (used to decide whether a warning is warranted).
        /// </summary>
        public static bool IsDifferentResolvedName(string serverDriverName, string resolvedLocalName)
        {
            return !string.Equals(serverDriverName.Trim(), resolvedLocalName.Trim(), StringComparison.OrdinalIgnoreCase);
        }
    }
}