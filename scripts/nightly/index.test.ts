import { describe, expect, it } from "bun:test";
import { execSync } from "node:child_process";
import { resolve } from "node:path";
import { readFileSync, unlinkSync } from "node:fs";
import { cleanGitEnv } from "./git";

const rootDir = resolve(import.meta.dirname, "../..");

function runCliCmd(cmdArgs: string): string {
  return execSync(`bun run scripts/nightly/index.ts ${cmdArgs}`, {
    cwd: rootDir,
    encoding: "utf-8",
    env: cleanGitEnv(),
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

  it("writes release notes to a valid multiline GitHub environment entry", () => {
    const tempFile = "test-notes-env-output.md";
    const envFile = "test-notes-github-env";
    try {
      runCliCmd(
        `generate-notes --version 3.4.0-nightly.1 --source-sha 4c9001d1234567890abcdef1234567890abcdef1 --out-file ${tempFile} --github-env-file ${envFile}`,
      );
      const notes = readFileSync(resolve(rootDir, tempFile), "utf-8");
      const envText = readFileSync(resolve(rootDir, envFile), "utf-8");
      const [header, ...lines] = envText.split("\n");
      const delimiter = header.match(/^RELEASE_BODY<<(.+)$/)?.[1];
      expect(delimiter).toBeDefined();
      const delimiterIndex = lines.indexOf(delimiter ?? "");
      expect(delimiterIndex).toBeGreaterThanOrEqual(0);
      expect(lines.slice(0, delimiterIndex).join("\n")).toBe(notes);
    } finally {
      for (const file of [tempFile, envFile]) {
        try {
          unlinkSync(resolve(rootDir, file));
        } catch {}
      }
    }
  });

  it("runs trigger with --dry-run and prints workflow dispatch preview", () => {
    const output = runCliCmd("trigger --dry-run");
    expect(output).toContain("ShaPrint Manual Nightly Release Dispatch");
    expect(output).toContain("workflow run nightly-desktop.yml --ref main");
  });
});
