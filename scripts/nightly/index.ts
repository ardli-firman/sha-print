import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { execGit } from "./git";
import { resolveAndVerifyCommit } from "./commit";
import {
  calculateNextNightlyVersion,
  formatNightlyTag,
  parseNightlyVersion,
} from "./version";
import { generateNightlyReleaseNotes } from "./notes";
import { determinePrunePlan, type GitHubReleaseItem } from "./prune";
import { applyProjectVersionAndEndpoint } from "./prepare-build";

export const DEFAULT_NIGHTLY_ENDPOINT =
  "https://github.com/ardli-firman/sha-print/releases/download/nightly/latest.json";
export const FEED_HOLDER_TAG = "nightly";

/**
 * Retrieves all git tags to find existing versions.
 */
export function getExistingTags(cwd?: string): string[] {
  try {
    const raw = execGit("git tag -l", cwd);
    return raw
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
  } catch {
    return [];
  }
}

/**
 * Retrieves current base version from origin/main's apps/desktop/package.json
 * falling back to local apps/desktop/package.json.
 */
export function getCurrentBaseVersion(rootDir: string = process.cwd()): string {
  try {
    const mainPkg = execGit("git show origin/main:apps/desktop/package.json", rootDir);
    return JSON.parse(mainPkg).version;
  } catch {
    const pkgPath = resolve(rootDir, "apps/desktop/package.json");
    const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
    return pkg.version;
  }
}

/**
 * CLI command handlers
 */
export async function runCli(args: string[]): Promise<void> {
  const command = args[2];
  const rootDir = resolve(import.meta.dirname, "../..");

  if (command === "resolve-commit") {
    const sourceSha = getArgValue(args, "--source-sha");
    const result = resolveAndVerifyCommit({
      cwd: rootDir,
      sourceSha: sourceSha || undefined,
      mainRef: "origin/main",
    });
    console.log(JSON.stringify(result));
    return;
  }

  if (command === "resolve-version") {
    const commitSha = getArgValue(args, "--commit-sha");
    const runNumberStr = getArgValue(args, "--run-number");
    const runNumber = runNumberStr ? parseInt(runNumberStr, 10) : undefined;

    // Read base version from main
    const currentMainBase = getCurrentBaseVersion(rootDir);

    // If commitSha was provided and differs, read its package.json
    let commitBase: string | undefined = undefined;
    if (commitSha) {
      try {
        const pkgRaw = execGit(`git show "${commitSha}:apps/desktop/package.json"`, rootDir);
        commitBase = JSON.parse(pkgRaw).version;
      } catch {
        commitBase = currentMainBase;
      }
    }

    const existingTags = getExistingTags(rootDir);
    const calculated = calculateNextNightlyVersion({
      currentMainBaseVersion: currentMainBase,
      commitBaseVersion: commitBase,
      existingTagsOrVersions: existingTags,
      runNumber,
    });

    console.log(JSON.stringify(calculated));
    return;
  }

  if (command === "prepare-build") {
    const version = getArgValue(args, "--version");
    if (!version) {
      throw new Error("--version is required for prepare-build");
    }
    const endpoint = getArgValue(args, "--endpoint") || DEFAULT_NIGHTLY_ENDPOINT;

    applyProjectVersionAndEndpoint({
      rootDir,
      version,
      endpoint,
    });
    console.log(`Updated project to version ${version} and endpoint ${endpoint}`);
    return;
  }

  if (command === "generate-notes") {
    const version = getArgValue(args, "--version");
    const sourceSha = getArgValue(args, "--source-sha");
    const settingsRisk = args.includes("--settings-risk");
    const outFile = getArgValue(args, "--out-file");

    if (!version || !sourceSha) {
      throw new Error("--version and --source-sha are required for generate-notes");
    }

    const notes = generateNightlyReleaseNotes({
      version,
      sourceSha,
      settingsCompatibilityRisk: settingsRisk,
    });

    if (outFile) {
      writeFileSync(resolve(rootDir, outFile), notes, "utf-8");
      console.log(`Wrote release notes to ${outFile}`);
    } else {
      console.log(notes);
    }
    return;
  }

  if (command === "update-feed") {
    const latestJsonPath = getArgValue(args, "--latest-json");
    const targetTag = getArgValue(args, "--tag");
    const feedTag = getArgValue(args, "--feed-tag") || FEED_HOLDER_TAG;

    if (!latestJsonPath || !targetTag) {
      throw new Error("--latest-json and --tag are required for update-feed");
    }

    const { updateNightlyFeed } = await import("./publish");
    await updateNightlyFeed({
      feedTag,
      targetTag,
      latestJsonPath: resolve(rootDir, latestJsonPath),
    });
    console.log(`Successfully updated nightly feed holder (${feedTag}) to point to ${targetTag}`);
    return;
  }

  if (command === "prune-releases") {
    const activeFeedTag = getArgValue(args, "--active-feed-tag");
    const retainCountStr = getArgValue(args, "--retain-count");
    const retainCount = retainCountStr ? parseInt(retainCountStr, 10) : 10;
    const feedTag = getArgValue(args, "--feed-tag") || FEED_HOLDER_TAG;

    if (!activeFeedTag) {
      throw new Error("--active-feed-tag is required for prune-releases");
    }

    const { pruneOldNightlyReleases } = await import("./publish");
    const count = await pruneOldNightlyReleases({
      activeFeedTag,
      retainCount,
      feedHolderTag: feedTag,
    });
    console.log(`Pruned ${count} old nightly release(s)`);
    return;
  }

  console.error(`Unknown command: ${command}`);
  process.exit(1);
}

function getArgValue(args: string[], flag: string): string | null {
  const idx = args.indexOf(flag);
  if (idx !== -1 && idx + 1 < args.length) {
    return args[idx + 1];
  }
  return null;
}

if (import.meta.main) {
  runCli(process.argv).catch((err) => {
    console.error("Nightly CLI error:", err);
    process.exit(1);
  });
}
