import pkg from "../../package.json";

/** Semantic version parsing and drift detection between Client and Nearby Servers. */

export const CLIENT_VERSION: string = pkg.version || "3.0.0";

export interface VersionDriftResult {
  hasDrift: boolean;
  isNewer: boolean;
  isMismatched: boolean;
  message?: string;
}

export function parseSemver(v: string): { major: number; minor: number; patch: number } | null {
  const match = v.trim().match(/^v?(\d+)\.(\d+)\.(\d+)/);
  if (!match) return null;
  return {
    major: parseInt(match[1], 10),
    minor: parseInt(match[2], 10),
    patch: parseInt(match[3], 10),
  };
}

export function compareSemver(a: string, b: string): number | null {
  const pa = parseSemver(a);
  const pb = parseSemver(b);
  if (!pa || !pb) return null;
  if (pa.major !== pb.major) return pa.major - pb.major;
  if (pa.minor !== pb.minor) return pa.minor - pb.minor;
  return pa.patch - pb.patch;
}

export function checkVersionDrift(
  serverVersion?: string | null,
  clientVersion: string = CLIENT_VERSION
): VersionDriftResult {
  if (!serverVersion || !serverVersion.trim()) {
    return { hasDrift: false, isNewer: false, isMismatched: false };
  }
  const parsedServer = parseSemver(serverVersion);
  const parsedClient = parseSemver(clientVersion);
  if (!parsedServer || !parsedClient) {
    const hasDrift = serverVersion.trim() !== clientVersion.trim();
    return {
      hasDrift,
      isNewer: false,
      isMismatched: hasDrift,
      message: hasDrift ? `Version drift (v${serverVersion})` : undefined,
    };
  }

  const isMismatched =
    parsedServer.major !== parsedClient.major || parsedServer.minor !== parsedClient.minor;
  const cmp = compareSemver(serverVersion, clientVersion);
  const isNewer = cmp !== null && cmp > 0;
  const hasDrift = isMismatched || isNewer;

  let message: string | undefined;
  if (isNewer) {
    message = `Newer server release (v${serverVersion})`;
  } else if (isMismatched) {
    message = `Version drift (v${serverVersion})`;
  }

  return { hasDrift, isNewer, isMismatched, message };
}
