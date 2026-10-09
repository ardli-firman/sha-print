import { describe, expect, it } from "bun:test";
import { resolveAndVerifyCommit } from "./commit";
import { execSync } from "node:child_process";

describe("commit resolution and verification", () => {
  it("resolves main HEAD when sourceSha is empty or omitted", () => {
    const result = resolveAndVerifyCommit({ mainRef: "HEAD" });
    expect(result.sha).toMatch(/^[0-9a-f]{40}$/);
    expect(result.shortSha).toBe(result.sha.slice(0, 7));
    expect(result.isHead).toBe(true);
  });

  it("resolves an older commit reachable from main", () => {
    // Get HEAD~1
    const headMinus1 = execSync("git rev-parse HEAD~1", {
      encoding: "utf-8",
      env: { ...process.env, GIT_CONFIG_COUNT: undefined },
    }).trim();

    const result = resolveAndVerifyCommit({
      sourceSha: headMinus1,
      mainRef: "HEAD",
    });

    expect(result.sha).toBe(headMinus1);
    expect(result.shortSha).toBe(headMinus1.slice(0, 7));
    expect(result.isHead).toBe(false);
  });

  it("accepts a short SHA if reachable from main", () => {
    const headMinus1 = execSync("git rev-parse HEAD~1", {
      encoding: "utf-8",
      env: { ...process.env, GIT_CONFIG_COUNT: undefined },
    }).trim();
    const short = headMinus1.slice(0, 7);

    const result = resolveAndVerifyCommit({
      sourceSha: short,
      mainRef: "HEAD",
    });

    expect(result.sha).toBe(headMinus1);
  });

  it("rejects an invalid git commit identifier", () => {
    expect(() =>
      resolveAndVerifyCommit({
        sourceSha: "not-a-real-commit-12345",
        mainRef: "HEAD",
      }),
    ).toThrow(/Invalid commit/i);
  });

  it("rejects a commit outside main history", () => {
    // Create an orphan commit in a temp repository or test mock
    // Using a fake 40-char SHA that does not exist in history
    expect(() =>
      resolveAndVerifyCommit({
        sourceSha: "0000000000000000000000000000000000000000",
        mainRef: "HEAD",
      }),
    ).toThrow();
  });
});
