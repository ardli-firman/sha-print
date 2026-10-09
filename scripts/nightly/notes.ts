export interface CommitSummary {
  sha: string;
  subject: string;
  body?: string;
}

export interface GenerateNotesOptions {
  version: string;
  sourceSha: string;
  settingsCompatibilityRisk?: boolean;
  breakingChanges?: string[];
  baseStableTag?: string;
}

const BREAKING_SUBJECT_RE = /^[a-z]+(?:\([^)]+\))?!:\s*(.+)$/i;
const BREAKING_BODY_RE = /BREAKING[ -]CHANGE:\s*(.+)/i;

/**
 * Inspects a list of commits and returns formatted descriptions for any commit
 * that declares a breaking change via Conventional Commits (`type!:` or `BREAKING CHANGE:`).
 */
export function detectBreakingChanges(commits: CommitSummary[]): string[] {
  const results: string[] = [];

  for (const commit of commits) {
    const subjectMatch = commit.subject.match(BREAKING_SUBJECT_RE);
    const bodyMatch = commit.body?.match(BREAKING_BODY_RE);

    if (subjectMatch || bodyMatch) {
      const shortSha = commit.sha.slice(0, 7);
      const link = `[\`${shortSha}\`](https://github.com/ardli-firman/sha-print/commit/${commit.sha})`;
      const detail = bodyMatch ? `${commit.subject} — *${bodyMatch[1].trim()}*` : commit.subject;
      results.push(`${link}: ${detail}`);
    }
  }

  return results;
}

/**
 * Generates release notes for a nightly prerelease according to ADR 0015.
 */
export function generateNightlyReleaseNotes(options: GenerateNotesOptions): string {
  const shortSha = options.sourceSha.slice(0, 7);
  const version = options.version.replace(/^v/, "");
  const breaking = options.breakingChanges ?? [];
  const hasBreaking = breaking.length > 0;
  const baseTagInfo = options.baseStableTag ? ` since \`${options.baseStableTag}\`` : "";

  let notes = `## ShaPrint Nightly Desktop (${version})

> ⚠️ **Test Build Warning**
> This is an automated manual test build from the nightly test channel.
> It is intended for testing and verification only and is **not** a stable release.

### Build Information
- **Prerelease Version**: \`${version}\`
- **Source Commit**: [\`${shortSha}\`](https://github.com/ardli-firman/sha-print/commit/${options.sourceSha}) (\`${options.sourceSha}\`)
- **Channel**: Dedicated Nightly Channel
`;

  if (hasBreaking) {
    notes += `
### 🚨 Breaking Changes Detected
Breaking changes were detected${baseTagInfo}:
${breaking.map((item) => `- ${item}`).join("\n")}

> **Compatibility Impact**: Stable and Nightly instances across this boundary may not be able to print to each other until both Client and Server are updated.
`;
  } else {
    notes += `
### ✅ Cross-Version & Print Compatibility
- **Breaking Changes**: None detected${baseTagInfo}.
- **Stable ↔ Nightly Printing**: Compatible (Stable Clients can print to Nightly Servers and vice versa).
`;
  }

  if (options.settingsCompatibilityRisk) {
    notes += `
### ⚠️ Settings Compatibility Risk
This nightly build introduces configuration or state changes that may not be backward-compatible with older stable versions.
If you return to a stable release after testing this build, you may need to reconfigure your printer sharing settings or Network Channel manually.
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
