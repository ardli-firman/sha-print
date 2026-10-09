export interface ParsedNightlyVersion {
  major: number;
  minor: number;
  patch: number;
  channel: string;
  build: number;
  raw: string;
}

export interface CalculateVersionOptions {
  currentMainBaseVersion: string;
  commitBaseVersion?: string;
  existingTagsOrVersions: string[];
  runNumber?: number;
}

export interface CalculatedNightlyVersion {
  version: string;
  tag: string;
  baseVersion: string;
  buildNumber: number;
}

/**
 * Normalizes a version string by removing leading 'v'.
 */
function cleanVersionString(v: string): string {
  return v.trim().replace(/^v/, "");
}

/**
 * Parses a nightly version or tag into SemVer components.
 * Matches: X.Y.Z-nightly.N (or with leading 'v').
 */
export function parseNightlyVersion(v: string): ParsedNightlyVersion | null {
  const cleaned = cleanVersionString(v);
  const match = cleaned.match(/^(\d+)\.(\d+)\.(\d+)-(nightly)\.(\d+)$/);
  if (!match) return null;

  return {
    major: parseInt(match[1], 10),
    minor: parseInt(match[2], 10),
    patch: parseInt(match[3], 10),
    channel: match[4],
    build: parseInt(match[5], 10),
    raw: cleaned,
  };
}

/**
 * Parses standard 3-part SemVer base (X.Y.Z).
 */
export function parseBaseSemver(v: string): { major: number; minor: number; patch: number } | null {
  const cleaned = cleanVersionString(v);
  const match = cleaned.match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!match) return null;
  return {
    major: parseInt(match[1], 10),
    minor: parseInt(match[2], 10),
    patch: parseInt(match[3], 10),
  };
}

/**
 * Compares two base SemVer versions (major, minor, patch).
 * Returns > 0 if a > b, < 0 if a < b, 0 if equal.
 */
export function compareBaseSemver(a: string, b: string): number {
  const pa = parseBaseSemver(a);
  const pb = parseBaseSemver(b);
  if (!pa || !pb) return 0;
  if (pa.major !== pb.major) return pa.major - pb.major;
  if (pa.minor !== pb.minor) return pa.minor - pb.minor;
  return pa.patch - pb.patch;
}

/**
 * Compares two nightly SemVer versions according to SemVer 2.0.0.
 * Major.Minor.Patch compared first, then prerelease build number.
 */
export function compareNightlyVersions(a: string, b: string): number {
  const pa = parseNightlyVersion(a);
  const pb = parseNightlyVersion(b);
  if (!pa && !pb) return 0;
  if (!pa) return -1;
  if (!pb) return 1;

  if (pa.major !== pb.major) return pa.major - pb.major;
  if (pa.minor !== pb.minor) return pa.minor - pb.minor;
  if (pa.patch !== pb.patch) return pa.patch - pb.patch;

  return pa.build - pb.build;
}

/**
 * Formats a nightly version string into a tag name.
 */
export function formatNightlyTag(version: string): string {
  const cleaned = cleanVersionString(version);
  return `v${cleaned}`;
}

/**
 * Calculates the next unique, strictly increasing SemVer nightly prerelease version.
 * Ensures:
 * 1. Base version is at least the highest base version among current main and previous nightlies.
 * 2. Prerelease build number is strictly greater than earlier nightlies with that base.
 * 3. Incorporates GitHub runNumber if supplied and greater.
 */
export function calculateNextNightlyVersion(
  options: CalculateVersionOptions,
): CalculatedNightlyVersion {
  const mainBase = cleanVersionString(options.currentMainBaseVersion).split("-")[0];
  const commitBase = options.commitBaseVersion
    ? cleanVersionString(options.commitBaseVersion).split("-")[0]
    : mainBase;

  // Extract all existing valid nightly versions
  const parsedNightlies = options.existingTagsOrVersions
    .map((tag) => parseNightlyVersion(tag))
    .filter((p): p is ParsedNightlyVersion => p !== null && p.channel === "nightly");

  // Determine highest existing nightly base
  let highestNightlyBase: string | null = null;
  let highestNightlyBuildForBase = 0;

  for (const n of parsedNightlies) {
    const base = `${n.major}.${n.minor}.${n.patch}`;
    if (!highestNightlyBase || compareBaseSemver(base, highestNightlyBase) > 0) {
      highestNightlyBase = base;
      highestNightlyBuildForBase = n.build;
    } else if (compareBaseSemver(base, highestNightlyBase) === 0) {
      if (n.build > highestNightlyBuildForBase) {
        highestNightlyBuildForBase = n.build;
      }
    }
  }

  // Target base version must be at least max(mainBase, highestNightlyBase)
  let targetBase = mainBase;
  if (highestNightlyBase && compareBaseSemver(highestNightlyBase, targetBase) > 0) {
    targetBase = highestNightlyBase;
  }

  // If commitBase happens to be higher than targetBase (unlikely, but safe)
  if (compareBaseSemver(commitBase, targetBase) > 0) {
    targetBase = commitBase;
  }

  // Calculate next build number for targetBase
  let nextBuild = 1;
  if (highestNightlyBase && compareBaseSemver(targetBase, highestNightlyBase) === 0) {
    nextBuild = highestNightlyBuildForBase + 1;
  }

  if (options.runNumber && options.runNumber >= nextBuild) {
    nextBuild = options.runNumber;
  }

  const version = `${targetBase}-nightly.${nextBuild}`;
  const tag = formatNightlyTag(version);

  return {
    version,
    tag,
    baseVersion: targetBase,
    buildNumber: nextBuild,
  };
}
