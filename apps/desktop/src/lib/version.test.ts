import { describe, expect, it } from "vitest";
import {
  checkVersionDrift,
  compareSemver,
  getReleaseChannel,
  isNightly,
  parseSemver,
} from "./version";

describe("version parsing and comparison", () => {
  it("parses valid semver versions", () => {
    expect(parseSemver("3.0.0")).toEqual({ major: 3, minor: 0, patch: 0 });
    expect(parseSemver("v3.1.2")).toEqual({ major: 3, minor: 1, patch: 2 });
    expect(parseSemver("invalid")).toBeNull();
  });

  it("compares semver versions accurately", () => {
    expect(compareSemver("3.1.0", "3.0.0")! > 0).toBe(true);
    expect(compareSemver("3.0.0", "3.1.0")! < 0).toBe(true);
    expect(compareSemver("3.0.0", "3.0.0")).toBe(0);
  });

  it("detects newer server release as version drift", () => {
    const drift = checkVersionDrift("3.1.0", "3.0.0");
    expect(drift.hasDrift).toBe(true);
    expect(drift.isNewer).toBe(true);
    expect(drift.message).toContain("Newer server release");
  });

  it("detects major/minor mismatch even if older", () => {
    const drift = checkVersionDrift("2.9.0", "3.0.0");
    expect(drift.hasDrift).toBe(true);
    expect(drift.isMismatched).toBe(true);
    expect(drift.isNewer).toBe(false);
    expect(drift.message).toContain("Version drift");
  });

  it("reports no drift when versions match", () => {
    const drift = checkVersionDrift("3.0.0", "3.0.0");
    expect(drift.hasDrift).toBe(false);
  });

  it("reports no drift and succeeds cleanly for older server without version", () => {
    expect(checkVersionDrift(null, "3.0.0").hasDrift).toBe(false);
    expect(checkVersionDrift(undefined, "3.0.0").hasDrift).toBe(false);
    expect(checkVersionDrift("", "3.0.0").hasDrift).toBe(false);
  });

  it("identifies nightly vs stable release channels", () => {
    expect(isNightly("3.4.0-nightly.1")).toBe(true);
    expect(isNightly("v3.4.0-nightly.15")).toBe(true);
    expect(isNightly("3.4.0")).toBe(false);
    expect(isNightly("v3.4.0")).toBe(false);
    expect(isNightly(null)).toBe(false);

    expect(getReleaseChannel("3.4.0-nightly.1")).toBe("nightly");
    expect(getReleaseChannel("3.4.0")).toBe("stable");
  });
});
