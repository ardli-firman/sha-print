import { describe, expect, it } from "bun:test";
import {
  canAdvanceFeed,
  prepareFeedMetadata,
  validateFeedUpdate,
  type UpdaterMetadata,
} from "./feed";

describe("nightly feed manager and validator", () => {
  const sampleMetadata: UpdaterMetadata = {
    version: "3.4.0-nightly.2",
    notes: "Nightly 2 notes",
    pub_date: "2026-10-10T00:00:00Z",
    platforms: {
      "windows-x86_64": {
        signature: "dW50cnVzdGVkIGNvbW1lbnQ6...",
        url: "https://github.com/ardli-firman/sha-print/releases/download/v3.4.0-nightly.2/ShaPrint.nsis.zip",
      },
    },
  };

  it("permits advancing feed when no current feed exists", () => {
    expect(canAdvanceFeed("3.4.0-nightly.1", null)).toBe(true);
    expect(canAdvanceFeed("3.4.0-nightly.1", undefined)).toBe(true);
  });

  it("permits advancing feed when new version is strictly greater than current feed", () => {
    expect(canAdvanceFeed("3.4.0-nightly.2", "3.4.0-nightly.1")).toBe(true);
    expect(canAdvanceFeed("3.5.0-nightly.1", "3.4.0-nightly.9")).toBe(true);
  });

  it("rejects advancing feed when new version is older or equal (prevents backwards movement)", () => {
    expect(canAdvanceFeed("3.4.0-nightly.1", "3.4.0-nightly.2")).toBe(false);
    expect(canAdvanceFeed("3.4.0-nightly.1", "3.4.0-nightly.1")).toBe(false);
  });

  it("validates full feed update structure", () => {
    const current: UpdaterMetadata = {
      version: "3.4.0-nightly.1",
      notes: "Nightly 1 notes",
      pub_date: "2026-10-09T00:00:00Z",
      platforms: {
        "windows-x86_64": {
          signature: "sig1",
          url: "https://example.com/asset1.zip",
        },
      },
    };

    const validResult = validateFeedUpdate({
      newMetadata: sampleMetadata,
      currentFeedMetadata: current,
    });
    expect(validResult.allowed).toBe(true);

    // Reject older version
    const olderMetadata = { ...sampleMetadata, version: "3.4.0-nightly.1" };
    const invalidResult = validateFeedUpdate({
      newMetadata: olderMetadata,
      currentFeedMetadata: current,
    });
    expect(invalidResult.allowed).toBe(false);
    expect(invalidResult.reason).toContain("cannot move feed backward");
  });

  it("rejects metadata missing platforms or signature", () => {
    const brokenMetadata: UpdaterMetadata = {
      version: "3.4.0-nightly.3",
      notes: "broken",
      pub_date: "2026-10-10T00:00:00Z",
      platforms: {},
    };

    const result = validateFeedUpdate({
      newMetadata: brokenMetadata,
      currentFeedMetadata: null,
    });
    expect(result.allowed).toBe(false);
    expect(result.reason).toContain("platforms");
  });

  it("ensures asset URLs reference the versioned prerelease tag", () => {
    const prepared = prepareFeedMetadata({
      rawMetadata: {
        version: "3.4.0-nightly.2",
        pub_date: "2026-10-10T00:00:00Z",
        platforms: {
          "windows-x86_64": {
            signature: "sig",
            url: "https://github.com/ardli-firman/sha-print/releases/download/v3.4.0-nightly.2/ShaPrint.zip",
          },
        },
      },
      targetTag: "v3.4.0-nightly.2",
    });

    expect(prepared.platforms["windows-x86_64"].url).toContain(
      "/releases/download/v3.4.0-nightly.2/",
    );
  });
});
