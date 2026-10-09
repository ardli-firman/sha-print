import { describe, expect, it } from "bun:test";
import { generateNightlyReleaseNotes } from "./notes";

describe("nightly release notes generator", () => {
  it("generates notes identifying test build, source SHA, and version", () => {
    const notes = generateNightlyReleaseNotes({
      version: "3.4.0-nightly.1",
      sourceSha: "4c9001d1234567890abcdef1234567890abcdef1",
    });

    expect(notes).toContain("Test Build");
    expect(notes).toContain("3.4.0-nightly.1");
    expect(notes).toContain("4c9001d1234567890abcdef1234567890abcdef1");
    expect(notes).toContain("4c9001d");
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
});
