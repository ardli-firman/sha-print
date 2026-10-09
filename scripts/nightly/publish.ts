import { execSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import {
  canAdvanceFeed,
  prepareFeedMetadata,
  validateFeedUpdate,
  type UpdaterMetadata,
} from "./feed";
import { determinePrunePlan, type GitHubReleaseItem, type PrunePlan } from "./prune";
import { parseNightlyVersion } from "./version";

export interface ReleaseClient {
  listReleases(): Promise<GitHubReleaseItem[]>;
  getFeedMetadata(feedTag: string): Promise<UpdaterMetadata | null>;
  uploadFeedAsset(feedTag: string, latestJsonPath: string): Promise<void>;
  deleteReleaseAndTag(tagName: string): Promise<void>;
}

function cleanGitEnv(): NodeJS.ProcessEnv {
  const env = { ...process.env };
  delete env.GIT_CONFIG_COUNT;
  delete env.GIT_CONFIG_VALUE_0;
  delete env.GIT_CONFIG_VALUE_1;
  return env;
}

function execGh(cmd: string, cwd?: string): string {
  return execSync(`gh ${cmd}`, {
    cwd: cwd || process.cwd(),
    encoding: "utf-8",
    env: cleanGitEnv(),
    stdio: ["pipe", "pipe", "pipe"],
  }).trim();
}

/**
 * Concrete ReleaseClient implementation backed by GitHub CLI (`gh`).
 */
export class GhCliReleaseClient implements ReleaseClient {
  private cwd?: string;

  constructor(cwd?: string) {
    this.cwd = cwd;
  }

  async listReleases(): Promise<GitHubReleaseItem[]> {
    try {
      const output = execGh("release list --limit 100 --json tagName,name,isPrerelease,isDraft", this.cwd);
      return JSON.parse(output) as GitHubReleaseItem[];
    } catch (err) {
      console.warn("Failed to list releases via gh:", err);
      return [];
    }
  }

  async getFeedMetadata(feedTag: string): Promise<UpdaterMetadata | null> {
    try {
      // Use gh release view or gh api to fetch feed asset content
      const output = execGh(`release view "${feedTag}" --json assets`, this.cwd);
      const parsed = JSON.parse(output);
      const hasLatestJson = parsed.assets?.some(
        (a: { name: string }) => a.name === "latest.json",
      );
      if (!hasLatestJson) return null;

      // Download latest.json to a temp directory or download via gh release download
      const tempDir = resolve(this.cwd || process.cwd(), ".gh-feed-temp");
      execGh(`release download "${feedTag}" -p "latest.json" -D "${tempDir}" --clobber`, this.cwd);
      const content = readFileSync(resolve(tempDir, "latest.json"), "utf-8");
      return JSON.parse(content) as UpdaterMetadata;
    } catch {
      return null;
    }
  }

  async uploadFeedAsset(feedTag: string, latestJsonPath: string): Promise<void> {
    // Check if feed release exists
    let feedExists = false;
    try {
      execGh(`release view "${feedTag}"`, this.cwd);
      feedExists = true;
    } catch {
      feedExists = false;
    }

    if (!feedExists) {
      // Create feed holder release
      execGh(
        `release create "${feedTag}" "${latestJsonPath}" --title "ShaPrint Nightly Feed" --notes "Dedicated update feed holder for ShaPrint nightly channel" --prerelease`,
        this.cwd,
      );
    } else {
      // Upload with clobber to overwrite latest.json
      execGh(`release upload "${feedTag}" "${latestJsonPath}" --clobber`, this.cwd);
    }
  }

  async deleteReleaseAndTag(tagName: string): Promise<void> {
    execGh(`release delete "${tagName}" --yes --cleanup-tag`, this.cwd);
  }
}

/**
 * Updates the dedicated nightly feed with the new build's updater metadata.
 * Validates metadata and guarantees feed cannot move backward.
 */
export async function updateNightlyFeed(options: {
  client?: ReleaseClient;
  feedTag?: string;
  targetTag: string;
  latestJsonPath: string;
}): Promise<void> {
  const client = options.client || new GhCliReleaseClient();
  const feedTag = options.feedTag || "nightly";

  if (!existsSync(options.latestJsonPath)) {
    throw new Error(`latest.json not found at: ${options.latestJsonPath}`);
  }

  const rawContent = readFileSync(options.latestJsonPath, "utf-8");
  const rawMetadata = JSON.parse(rawContent) as UpdaterMetadata;

  const preparedMetadata = prepareFeedMetadata({
    rawMetadata,
    targetTag: options.targetTag,
  });

  const currentFeed = await client.getFeedMetadata(feedTag);
  const validation = validateFeedUpdate({
    newMetadata: preparedMetadata,
    currentFeedMetadata: currentFeed,
  });

  if (!validation.allowed) {
    throw new Error(`Feed update rejected: ${validation.reason}`);
  }

  // Upload to feed holder release
  await client.uploadFeedAsset(feedTag, options.latestJsonPath);
}

/**
 * Executes pruning according to a determined PrunePlan.
 * Defensively ensures neither feed holder nor stable releases are ever deleted.
 */
export async function executePrunePlan(
  plan: PrunePlan,
  client: ReleaseClient,
): Promise<number> {
  let deletedCount = 0;

  for (const rel of plan.toDelete) {
    // Defense: skip feed holder
    if (rel.tagName === "nightly" || rel.tagName === "nightly-feed") {
      continue;
    }

    // Defense: skip stable / non-prerelease
    if (!rel.isPrerelease) {
      continue;
    }

    // Defense: verify it is a valid nightly version tag
    if (!parseNightlyVersion(rel.tagName)) {
      continue;
    }

    try {
      await client.deleteReleaseAndTag(rel.tagName);
      deletedCount++;
    } catch (err) {
      console.warn(`Failed to delete release ${rel.tagName}:`, err);
    }
  }

  return deletedCount;
}

/**
 * High-level function to prune older nightly releases.
 */
export async function pruneOldNightlyReleases(options: {
  client?: ReleaseClient;
  activeFeedTag: string;
  retainCount?: number;
  feedHolderTag?: string;
}): Promise<number> {
  const client = options.client || new GhCliReleaseClient();
  const releases = await client.listReleases();

  const plan = determinePrunePlan({
    releases,
    activeFeedTag: options.activeFeedTag,
    retainCount: options.retainCount ?? 10,
    feedHolderTag: options.feedHolderTag || "nightly",
  });

  return executePrunePlan(plan, client);
}
