using System.Collections.Generic;
using ShaPrint.Platform.Windows;
using Xunit;

namespace ShaPrint.Tests
{
    /// <summary>
    /// Tests for DriverNameResolver — resolves the server-advertised driver name (hint)
    /// against locally installed drivers (issue #21: Epson L120 ESC/P-R name mismatch).
    /// </summary>
    public class DriverNameResolverTests
    {
        // ── Normalize ────────────────────────────────────────────────────────────

        [Theory]
        [InlineData("EPSON L120 Series", "epson l120 series")]
        [InlineData("EPSON L120 Series ESC/P-R", "epson l120 series")]
        [InlineData("EPSON L120 Series (Copy 1)", "epson l120 series")]
        [InlineData("EPSON L120 Series (Copy 1) ESC/P-R", "epson l120 series")]
        [InlineData("  EPSON   L120  Series  ", "epson l120 series")]
        [InlineData(null, "")]
        [InlineData("", "")]
        public void Normalize_StripsSuffixesAndCollapses(string? input, string expected)
        {
            Assert.Equal(expected, DriverNameResolver.Normalize(input));
        }

        // ── Resolve: exact ───────────────────────────────────────────────────────

        [Fact]
        public void Resolve_ExactMatch_ReturnsLocalDriver()
        {
            var local = new List<string> { "EPSON L120 Series", "Microsoft Print to PDF" };
            Assert.Equal("EPSON L120 Series", DriverNameResolver.Resolve("EPSON L120 Series", local));
        }

        [Fact]
        public void Resolve_ExactMatch_IsCaseInsensitive()
        {
            var local = new List<string> { "epson l120 series" };
            Assert.Equal("epson l120 series", DriverNameResolver.Resolve("EPSON L120 Series", local));
        }

        // ── Resolve: fuzzy (the Epson L120 ESC/P-R scenario) ─────────────────────

        [Fact]
        public void Resolve_ServerHasEscPR_Suffix_LocalDoesNot_MatchesLocal()
        {
            // Server advertises "EPSON L120 Series ESC/P-R", local machine has plain "EPSON L120 Series".
            var local = new List<string> { "EPSON L120 Series", "Microsoft Print to PDF" };
            var result = DriverNameResolver.Resolve("EPSON L120 Series ESC/P-R", local);
            Assert.Equal("EPSON L120 Series", result);
        }

        [Fact]
        public void Resolve_ServerHasNoSuffix_LocalHasEscPR_MatchesLocal()
        {
            var local = new List<string> { "EPSON L120 Series ESC/P-R", "Generic / Text Only" };
            var result = DriverNameResolver.Resolve("EPSON L120 Series", local);
            Assert.Equal("EPSON L120 Series ESC/P-R", result);
        }

        [Fact]
        public void Resolve_LocalHasCopySuffix_Matches()
        {
            var local = new List<string> { "EPSON L120 Series (Copy 1)" };
            var result = DriverNameResolver.Resolve("EPSON L120 Series", local);
            Assert.Equal("EPSON L120 Series (Copy 1)", result);
        }

        [Fact]
        public void Resolve_TokenContainment_MultipleTokens_Matches()
        {
            var local = new List<string> { "EPSON L120 Series ESC/P-R" };
            var result = DriverNameResolver.Resolve("EPSON L120 Series", local);
            Assert.Equal("EPSON L120 Series ESC/P-R", result);
        }

        // ── Resolve: no match ────────────────────────────────────────────────────

        [Fact]
        public void Resolve_NoMatch_ReturnsNull()
        {
            var local = new List<string> { "Microsoft Print to PDF", "Generic / Text Only" };
            Assert.Null(DriverNameResolver.Resolve("EPSON L120 Series", local));
        }

        [Fact]
        public void Resolve_EmptyLocalList_ReturnsNull()
        {
            Assert.Null(DriverNameResolver.Resolve("EPSON L120 Series", new List<string>()));
        }

        [Fact]
        public void Resolve_NullHint_ReturnsNull()
        {
            var local = new List<string> { "EPSON L120 Series" };
            Assert.Null(DriverNameResolver.Resolve(null, local));
        }

        [Fact]
        public void Resolve_DifferentModel_DoesNotMatch()
        {
            // "EPSON L3110" should not match "EPSON L120".
            var local = new List<string> { "EPSON L3110 Series" };
            Assert.Null(DriverNameResolver.Resolve("EPSON L120 Series", local));
        }

        // ── Resolve: enhanced scoring (issue #21 enhancement) ─────────────────────

        [Fact]
        public void Resolve_MultipleCandidates_PicksModelMatchingOne()
        {
            // Local store has both an L120 and an L3110; hint is L120 → must pick L120.
            var local = new List<string> { "EPSON L3110 Series ESC/P-R", "EPSON L120 Series ESC/P-R" };
            var result = DriverNameResolver.Resolve("EPSON L120 Series", local);
            Assert.Equal("EPSON L120 Series ESC/P-R", result);
        }

        [Fact]
        public void Resolve_ModelTokenDifference_DoesNotConfuseWf2850WithOther()
        {
            var local = new List<string> { "EPSON WF-2850 Series" };
            Assert.Null(DriverNameResolver.Resolve("EPSON L120 Series", local));
        }

        [Fact]
        public void Resolve_HpModel_SpecificPick()
        {
            var local = new List<string> { "HP LaserJet P1102", "HP Deskjet 4150" };
            var result = DriverNameResolver.Resolve("HP LaserJet P1102 Series", local);
            Assert.Equal("HP LaserJet P1102", result);
        }

        [Fact]
        public void Resolve_EpsonCopySuffix_StillMatch()
        {
            var local = new List<string> { "EPSON L120 Series (Copy 1) ESC/P-R" };
            var result = DriverNameResolver.Resolve("EPSON L120 Series", local);
            Assert.Equal("EPSON L120 Series (Copy 1) ESC/P-R", result);
        }

        [Fact]
        public void Resolve_ExactCopySuffix_PrefersExplicit()
        {
            var local = new List<string> { "EPSON L120 Series (Copy 1)", "EPSON L120 Series" };
            // Exact normalized equality wins over token scoring.
            Assert.Equal("EPSON L120 Series", DriverNameResolver.Resolve("EPSON L120 Series", local));
        }

        [Fact]
        public void Resolve_MultiTokenModel_PrefersDigitToken()
        {
            // "EPSON Expression XP-550" vs "EPSON L120" — hint is XP-550; must NOT match L120
            // and must pick the digit-bearing model token (xp-550) over the longer word (expression).
            var local = new List<string> { "EPSON Expression XP-550" };
            Assert.Equal("EPSON Expression XP-550", DriverNameResolver.Resolve("EPSON XP-550", local));
        }

        [Fact]
        public void Resolve_MultiTokenModel_DoesNotCrossMatch()
        {
            var local = new List<string> { "EPSON Expression XP-550" };
            Assert.Null(DriverNameResolver.Resolve("EPSON L120", local));
        }

        // ── MatchesFilter ───────────────────────────────────────────────────────

        [Fact]
        public void MatchesFilter_EmptyFilter_MatchesAll()
        {
            Assert.True(DriverNameResolver.MatchesFilter("EPSON L120 Series", ""));
            Assert.True(DriverNameResolver.MatchesFilter("EPSON L120 Series", null));
            Assert.True(DriverNameResolver.MatchesFilter("EPSON L120 Series", "   "));
        }

        [Fact]
        public void MatchesFilter_Substring_CaseInsensitive()
        {
            Assert.True(DriverNameResolver.MatchesFilter("EPSON L120 Series", "l120"));
            Assert.True(DriverNameResolver.MatchesFilter("EPSON L120 Series", "L120"));
            Assert.True(DriverNameResolver.MatchesFilter("EPSON L120 Series", "series"));
        }

        [Fact]
        public void MatchesFilter_NoMatch_ReturnsFalse()
        {
            Assert.False(DriverNameResolver.MatchesFilter("EPSON L120 Series", "l3110"));
            Assert.False(DriverNameResolver.MatchesFilter("EPSON L120 Series", "canon"));
        }

        [Fact]
        public void MatchesFilter_EmptyDriver_ReturnsFalse()
        {
            Assert.False(DriverNameResolver.MatchesFilter(null, "l120"));
            Assert.False(DriverNameResolver.MatchesFilter("", "l120"));
            Assert.False(DriverNameResolver.MatchesFilter("  ", "l120"));
        }

        // ── IsDifferentResolvedName ──────────────────────────────────────────────

        [Fact]
        public void IsDifferentResolvedName_True_WhenNamesDiffer()
        {
            Assert.True(DriverNameResolver.IsDifferentResolvedName("EPSON L120 Series ESC/P-R", "EPSON L120 Series"));
        }

        [Fact]
        public void IsDifferentResolvedName_False_WhenSame()
        {
            Assert.False(DriverNameResolver.IsDifferentResolvedName("EPSON L120 Series", "EPSON L120 Series"));
        }
    }
}
