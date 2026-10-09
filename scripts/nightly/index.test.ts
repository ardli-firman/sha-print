import { describe, expect, it } from "bun:test";
import { execSync } from "node:child_process";
import { resolve } from "node:path";
import { readFileSync, unlinkSync } from "node:fs";

const rootDir = resolve(import.meta.dirname, "../..");

function runCliCmd(cmdArgs: string): string {
  const env = { ...process.env };
  delete env.GIT_CONFIG_COUNT;
  delete env.GIT_CONFIG_VALUE_0;
  delete env.GIT_CONFIG_VALUE_1;

  return execSync(`bun run scripts/nightly/index.ts ${cmdArgs}`, {
    cwd: rootDir,
    encoding: "utf-8",
    env,
  }).trim();
}

describe("nightly CLI commands", () => {
  it("runs resolve-commit with default HEAD", () => {
    const output = runCliCmd("resolve-commit");
    const parsed = JSON.parse(output);
    expect(parsed.sha).toMatch(/^[0-9a-f]{40}$/);
    expect(parsed.shortSha).toHaveLength(7);
  });

  it("runs resolve-version and outputs valid nightly version", () => {
    const output = runCliCmd("resolve-version --run-number 42");
    const parsed = JSON.parse(output);
    expect(parsed.version).toContain("-nightly.");
    expect(parsed.tag).toBe(`v${parsed.version}`);
    expect(parsed.buildNumber).toBeGreaterThanOrEqual(42);
  });

  it("runs generate-notes to an output file", () => {
    const tempFile = "test-notes-output.md";
    try {
      runCliCmd(
        `generate-notes --version 3.4.0-nightly.1 --source-sha 4c9001d1234567890abcdef1234567890abcdef1 --settings-risk --out-file ${tempFile}`,
      );
      const content = readFileSync(resolve(rootDir, tempFile), "utf-8");
      expect(content).toContain("Test Build");
      expect(content).toContain("Settings Compatibility Risk");
      expect(content).toContain("3.4.0-nightly.1");
    } finally {
      try {
        unlinkSync(resolve(rootDir, tempFile));
      } catch {}
    }
  });
});
