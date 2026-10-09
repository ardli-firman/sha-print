import { compareNightlyVersions, parseNightlyVersion } from "./version";

export interface GitHubReleaseItem {
  id?: number | string;
  tagName: string;
  name?: string;
  isPrerelease: boolean;
  isDraft?: boolean;
}

export interface DeterminePrunePlanOptions {
  releases: GitHubReleaseItem[];
  feedHolderTag?: string;
  activeFeedTag?: string;
  retainCount?: number;
}

export interface PrunePlan {
  toRetain: GitHubReleaseItem[];
  toDelete: GitHubReleaseItem[];
  feedHolder?: GitHubReleaseItem;
  stableReleases: GitHubReleaseItem[];
}

/**
 * Determines which releases to retain and which to prune.
 * Retains:
 * - The feed holder release (e.g. tag "nightly")
 * - All stable releases and tags
 * - The release currently referenced by the active feed
 * - The top `retainCount` (default: 10) newest versioned nightly prereleases
 * Prunes:
 * - Versioned nightly prereleases beyond the top `retainCount` that are not the active feed
 */
export function determinePrunePlan(options: DeterminePrunePlanOptions): PrunePlan {
  const feedHolderTag = options.feedHolderTag || "nightly";
  const activeFeedTag = options.activeFeedTag?.trim();
  const retainCount = options.retainCount ?? 10;

  let feedHolder: GitHubReleaseItem | undefined;
  const stableReleases: GitHubReleaseItem[] = [];
  const versionedNightlies: GitHubReleaseItem[] = [];

  for (const rel of options.releases) {
    if (rel.tagName === feedHolderTag) {
      feedHolder = rel;
      continue;
    }

    const parsed = parseNightlyVersion(rel.tagName);
    if (!parsed || !rel.isPrerelease) {
      // Not a versioned nightly prerelease -> considered stable / non-nightly
      stableReleases.push(rel);
      continue;
    }

    versionedNightlies.push(rel);
  }

  // Sort versioned nightlies descending (newest first)
  versionedNightlies.sort((a, b) => compareNightlyVersions(b.tagName, a.tagName));

  const toRetain: GitHubReleaseItem[] = [];
  const toDelete: GitHubReleaseItem[] = [];

  for (let i = 0; i < versionedNightlies.length; i++) {
    const rel = versionedNightlies[i];
    const isWithinTopN = i < retainCount;
    const isActiveFeed = activeFeedTag && rel.tagName === activeFeedTag;

    if (isWithinTopN || isActiveFeed) {
      toRetain.push(rel);
    } else {
      toDelete.push(rel);
    }
  }

  return {
    toRetain,
    toDelete,
    feedHolder,
    stableReleases,
  };
}
