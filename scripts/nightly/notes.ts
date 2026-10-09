export interface GenerateNotesOptions {
  version: string;
  sourceSha: string;
  shortSha?: string;
  isHead?: boolean;
  settingsCompatibilityRisk?: boolean;
  customNotes?: string;
}

/**
 * Generates release notes for a nightly prerelease according to ADR 0015.
 */
export function generateNightlyReleaseNotes(options: GenerateNotesOptions): string {
  const shortSha = options.shortSha || options.sourceSha.slice(0, 7);
  const version = options.version.replace(/^v/, "");

  let notes = `## ShaPrint Nightly Desktop (${version})

> ⚠️ **Test Build Warning**
> This is an automated manual test build from the nightly test channel.
> It is intended for testing and verification only and is **not** a stable release.

### Build Information
- **Prerelease Version**: \`${version}\`
- **Source Commit**: [\`${shortSha}\`](https://github.com/ardli-firman/sha-print/commit/${options.sourceSha}) (\`${options.sourceSha}\`)
- **Channel**: Dedicated Nightly Channel
`;

  if (options.settingsCompatibilityRisk) {
    notes += `
### ⚠️ Settings Compatibility Risk
This nightly build introduces configuration or state changes that may not be backward-compatible with older stable versions.
If you return to a stable release after testing this build, you may need to reconfigure your printer sharing settings or Network Channel manually.
`;
  }

  if (options.customNotes?.trim()) {
    notes += `
### Notes
${options.customNotes.trim()}
`;
  }

  notes += `
### Channel & Update Behavior
- **Installation**: Installing this nightly will replace your existing ShaPrint installation. It retains your current application identity, app data, fixed service ports, and update verification.
- **Updates**: Installed nightly builds receive subsequent test builds from the dedicated nightly update feed.
- **Returning to Stable**: To return to the stable channel, download and install the latest stable release manually from [ShaPrint Releases](https://github.com/ardli-firman/sha-print/releases/latest).
`;

  return notes.trim();
}
