import { describe, expect, it } from "bun:test";
import { detectBreakingChanges, generateNightlyReleaseNotes } from "./notes";

describe("nightly release notes generator", () => {
  it("generates notes identifying test build, source SHA, version, and compatibility", () => {
    const notes = generateNightlyReleaseNotes({
      version: "3.4.0-nightly.1",
      sourceSha: "4c9001d1234567890abcdef1234567890abcdef1",
      baseStableTag: "v3.4.0",
    });

    expect(notes).toContain("Test Build");
    expect(notes).toContain("3.4.0-nightly.1");
    expect(notes).toContain("4c9001d1234567890abcdef1234567890abcdef1");
    expect(notes).toContain("4c9001d");
    expect(notes).toContain("Cross-Version & Print Compatibility");
    expect(notes).toContain("None detected since `v3.4.0`");
    expect(notes).toContain("Returning to Stable");
  });

  it("includes settings compatibility risk warning when flagged", () => {
    const notes = generateNightlyReleaseNotes({
      version: "3.4.0-nightly.2",
      sourceSha: "4c9001d1234567890abcdef1234567890abcdef1",
      settingsCompatibilityRisk: true,
    });

    expect(notes).toContain("Settings Compatibility Risk");
    expect(notes).toContain("reconfigure");
  });

  it("omits compatibility warning when risk is false or omitted", () => {
    const notes = generateNightlyReleaseNotes({
      version: "3.4.0-nightly.2",
      sourceSha: "4c9001d1234567890abcdef1234567890abcdef1",
      settingsCompatibilityRisk: false,
    });

    expect(notes).not.toContain("Settings Compatibility Risk");
  });

  it("detects Conventional Commit breaking changes and renders breaking changes warning", () => {
    const breaking = detectBreakingChanges([
      {
        sha: "aaaaaaa111111111111111111111111111111111",
        subject: "feat(desktop): add UI badge",
      },
      {
        sha: "bbbbbbb222222222222222222222222222222222",
        subject: "feat(ports)!: migrate IPPS server port to 48631",
      },
      {
        sha: "ccccccc333333333333333333333333333333333",
        subject: "refactor(ipps): update auth header",
        body: "BREAKING CHANGE: older clients must update to use new auth token format",
      },
    ]);

    expect(breaking).toHaveLength(2);
    expect(breaking[0]).toContain("bbbbbbb");
    expect(breaking[0]).toContain("feat(ports)!: migrate IPPS server port to 48631");
    expect(breaking[1]).toContain("ccccccc");
    expect(breaking[1]).toContain("older clients must update");

    const notes = generateNightlyReleaseNotes({
      version: "3.4.0-nightly.3",
      sourceSha: "ccccccc333333333333333333333333333333333",
      baseStableTag: "v3.4.0",
      breakingChanges: breaking,
    });

    expect(notes).toContain("Breaking Changes Detected");
    expect(notes).toContain("since `v3.4.0`");
    expect(notes).toContain("feat(ports)!: migrate IPPS server port to 48631");
    expect(notes).not.toContain("None detected");
  });
});
