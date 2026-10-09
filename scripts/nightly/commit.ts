import { execGit } from "./git";

export interface CommitResolutionOptions {
  cwd?: string;
  sourceSha?: string;
  mainRef?: string;
}

export interface CommitResolutionResult {
  sha: string;
  shortSha: string;
  isHead: boolean;
}

/**
 * Resolves the target commit SHA and verifies it is reachable from main.
 * If sourceSha is empty or omitted, defaults to mainRef (default: origin/main or main or HEAD).
 */
export function resolveAndVerifyCommit(
  options: CommitResolutionOptions = {},
): CommitResolutionResult {
  const cwd = options.cwd || process.cwd();
  const mainRef = options.mainRef || "origin/main";

  // First, verify mainRef exists or fallback to main or HEAD
  let targetMainRef = mainRef;
  try {
    execGit(`git rev-parse --verify "${targetMainRef}^{commit}"`, cwd);
  } catch {
    if (mainRef === "origin/main") {
      try {
        execGit(`git rev-parse --verify "main^{commit}"`, cwd);
        targetMainRef = "main";
      } catch {
        targetMainRef = "HEAD";
      }
    } else {
      throw new Error(`Invalid main reference: ${mainRef}`);
    }
  }

  const mainHeadSha = execGit(`git rev-parse "${targetMainRef}^{commit}"`, cwd);

  const rawSha = options.sourceSha?.trim();
  if (!rawSha) {
    return {
      sha: mainHeadSha,
      shortSha: mainHeadSha.slice(0, 7),
      isHead: true,
    };
  }

  // Verify the given SHA is a valid commit object
  let resolvedSha: string;
  try {
    resolvedSha = execGit(`git rev-parse --verify "${rawSha}^{commit}"`, cwd);
  } catch {
    throw new Error(`Invalid commit reference: ${rawSha}`);
  }

  // Verify that resolvedSha is an ancestor of (reachable from) targetMainRef
  try {
    execGit(`git merge-base --is-ancestor "${resolvedSha}" "${targetMainRef}"`, cwd);
  } catch {
    throw new Error(
      `Commit ${resolvedSha} (${rawSha}) is outside the history of ${targetMainRef} (${mainHeadSha})`,
    );
  }

  const isHead = resolvedSha === mainHeadSha;

  return {
    sha: resolvedSha,
    shortSha: resolvedSha.slice(0, 7),
    isHead,
  };
}
