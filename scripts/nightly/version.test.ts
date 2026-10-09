import { describe, expect, it } from "bun:test";
import {
  calculateNextNightlyVersion,
  compareNightlyVersions,
  formatNightlyTag,
  parseNightlyVersion,
} from "./version";

describe("nightly version calculation and comparison", () => {
  it("parses valid nightly versions and tags", () => {
    const parsed = parseNightlyVersion("3.4.0-nightly.1");
    expect(parsed).toEqual({
      major: 3,
      minor: 4,
      patch: 0,
      channel: "nightly",
      build: 1,
      raw: "3.4.0-nightly.1",
    });

    const parsedTag = parseNightlyVersion("v3.4.0-nightly.2");
    expect(parsedTag?.build).toBe(2);

    expect(parseNightlyVersion("3.4.0")).toBeNull();
    expect(parseNightlyVersion("v3.4.0")).toBeNull();
    expect(parseNightlyVersion("v1.6.3-beta.10")).toBeNull();
  });

  it("compares nightly versions correctly according to SemVer 2.0.0", () => {
    expect(compareNightlyVersions("3.4.0-nightly.1", "3.4.0-nightly.2")).toBeLessThan(0);
    expect(compareNightlyVersions("3.4.0-nightly.2", "3.4.0-nightly.1")).toBeGreaterThan(0);
    expect(compareNightlyVersions("3.4.0-nightly.1", "3.4.0-nightly.1")).toBe(0);

    // Major/minor differences
    expect(compareNightlyVersions("3.3.1-nightly.5", "3.4.0-nightly.1")).toBeLessThan(0);
    expect(compareNightlyVersions("3.5.0-nightly.1", "3.4.0-nightly.99")).toBeGreaterThan(0);
  });

  it("calculates first nightly version when no prior nightlies exist", () => {
    const result = calculateNextNightlyVersion({
      currentMainBaseVersion: "3.4.0",
      commitBaseVersion: "3.4.0",
      existingTagsOrVersions: ["v3.4.0", "v3.3.1"],
    });

    expect(result.version).toBe("3.4.0-nightly.1");
    expect(result.tag).toBe("v3.4.0-nightly.1");
  });

  it("increments counter over existing nightly releases", () => {
    const result = calculateNextNightlyVersion({
      currentMainBaseVersion: "3.4.0",
      commitBaseVersion: "3.4.0",
      existingTagsOrVersions: ["v3.4.0-nightly.1", "v3.4.0-nightly.2"],
    });

    expect(result.version).toBe("3.4.0-nightly.3");
    expect(result.tag).toBe("v3.4.0-nightly.3");
  });

  it("ensures a build of an older historical commit is strictly newer than earlier nightlies", () => {
    // Commit being built was from version 3.3.0
    // But previous nightly was 3.4.0-nightly.2
    const result = calculateNextNightlyVersion({
      currentMainBaseVersion: "3.4.0",
      commitBaseVersion: "3.3.0",
      existingTagsOrVersions: ["v3.4.0-nightly.1", "v3.4.0-nightly.2"],
    });

    // Version must be greater than 3.4.0-nightly.2
    expect(compareNightlyVersions(result.version, "3.4.0-nightly.2")).toBeGreaterThan(0);
    expect(result.version).toBe("3.4.0-nightly.3");
  });

  it("handles base version bump on main correctly", () => {
    // main is now 3.5.0, older nightlies were on 3.4.0
    const result = calculateNextNightlyVersion({
      currentMainBaseVersion: "3.5.0",
      commitBaseVersion: "3.5.0",
      existingTagsOrVersions: ["v3.4.0-nightly.5"],
    });

    expect(compareNightlyVersions(result.version, "3.4.0-nightly.5")).toBeGreaterThan(0);
    expect(result.version).toBe("3.5.0-nightly.1");
  });

  it("uses workflow run number if it is greater than the next counter", () => {
    const result = calculateNextNightlyVersion({
      currentMainBaseVersion: "3.4.0",
      commitBaseVersion: "3.4.0",
      existingTagsOrVersions: ["v3.4.0-nightly.2"],
      runNumber: 15,
    });

    expect(result.version).toBe("3.4.0-nightly.15");
  });

  it("formats tag correctly", () => {
    expect(formatNightlyTag("3.4.0-nightly.1")).toBe("v3.4.0-nightly.1");
  });
});
