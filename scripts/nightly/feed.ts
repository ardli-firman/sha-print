import { compareNightlyVersions } from "./version";

export interface PlatformMetadata {
  signature: string;
  url: string;
}

export interface UpdaterMetadata {
  version: string;
  notes?: string;
  pub_date: string;
  platforms: Record<string, PlatformMetadata>;
}

export interface ValidateFeedUpdateOptions {
  newMetadata: UpdaterMetadata;
  currentFeedMetadata?: UpdaterMetadata | null;
}

export interface ValidateFeedUpdateResult {
  allowed: boolean;
  reason?: string;
}

export interface PrepareFeedMetadataOptions {
  rawMetadata: UpdaterMetadata;
  targetTag: string;
}

/**
 * Checks whether the candidate version is strictly greater than the current feed version.
 * If there is no current feed version, advancement is always permitted.
 */
export function canAdvanceFeed(
  newVersion: string,
  currentFeedVersion?: string | null,
): boolean {
  if (!currentFeedVersion) {
    return true;
  }
  return compareNightlyVersions(newVersion, currentFeedVersion) > 0;
}

/**
 * Validates updater metadata structure and ensures that advancing the feed
 * will not move the version backwards.
 */
export function validateFeedUpdate(
  options: ValidateFeedUpdateOptions,
): ValidateFeedUpdateResult {
  const { newMetadata, currentFeedMetadata } = options;

  if (!newMetadata.version) {
    return { allowed: false, reason: "Metadata missing version field" };
  }

  const platforms = Object.keys(newMetadata.platforms || {});
  if (platforms.length === 0) {
    return { allowed: false, reason: "Metadata platforms dictionary is empty" };
  }

  for (const plat of platforms) {
    const p = newMetadata.platforms[plat];
    if (!p.signature || !p.signature.trim()) {
      return { allowed: false, reason: `Platform ${plat} is missing signature` };
    }
    if (!p.url || !p.url.trim()) {
      return { allowed: false, reason: `Platform ${plat} is missing url` };
    }
  }

  if (currentFeedMetadata?.version) {
    const isNewer = canAdvanceFeed(newMetadata.version, currentFeedMetadata.version);
    if (!isNewer) {
      return {
        allowed: false,
        reason: `New version ${newMetadata.version} is not newer than current feed version ${currentFeedMetadata.version}; cannot move feed backward`,
      };
    }
  }

  return { allowed: true };
}

/**
 * Normalizes metadata URLs to make sure they point to the published versioned prerelease tag assets.
 */
export function prepareFeedMetadata(
  options: PrepareFeedMetadataOptions,
): UpdaterMetadata {
  const { rawMetadata, targetTag } = options;
  const tag = targetTag.startsWith("v") ? targetTag : `v${targetTag}`;

  const platforms: Record<string, PlatformMetadata> = {};
  for (const [key, val] of Object.entries(rawMetadata.platforms || {})) {
    let url = val.url;
    // Ensure URL points to the targetTag
    if (url.includes("/releases/download/")) {
      url = url.replace(/\/releases\/download\/[^/]+\//, `/releases/download/${tag}/`);
    }
    platforms[key] = {
      signature: val.signature,
      url,
    };
  }

  return {
    ...rawMetadata,
    platforms,
  };
}
