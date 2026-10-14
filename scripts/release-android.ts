import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { execSync } from "node:child_process";

const rootDir = resolve(import.meta.dirname, "..");
const gradlePropsPath = resolve(rootDir, "apps/android/gradle.properties");

type BumpType = "patch" | "minor" | "major";

function bumpVersion(current: string, type: BumpType): string {
  const parts = current.split(".").map((n) => parseInt(n, 10));
  if (parts.length < 3 || parts.some(isNaN)) {
    throw new Error(`Invalid current version: ${current}`);
  }
  let [major, minor, patch] = parts;
  if (type === "patch") patch += 1;
  else if (type === "minor") {
    minor += 1;
    patch = 0;
  } else if (type === "major") {
    major += 1;
    minor = 0;
    patch = 0;
  }
  return `${major}.${minor}.${patch}`;
}

const bumpType = (process.argv[2] as BumpType) || "patch";
if (!["patch", "minor", "major"].includes(bumpType)) {
  console.error("Usage: bun run scripts/release-android.ts [patch|minor|major]");
  process.exit(1);
}

// 1. Read apps/android/gradle.properties
let gradleProps = readFileSync(gradlePropsPath, "utf-8");

const versionCodeMatch = gradleProps.match(/^VERSION_CODE=(\d+)/m);
const versionNameMatch = gradleProps.match(/^VERSION_NAME=([^\r\n]+)/m);

if (!versionCodeMatch || !versionNameMatch) {
  throw new Error("Could not find VERSION_CODE or VERSION_NAME in apps/android/gradle.properties");
}

const currentVersionCode = parseInt(versionCodeMatch[1], 10);
const currentVersionName = versionNameMatch[1].trim();

const nextVersionCode = currentVersionCode + 1;
const nextVersionName = bumpVersion(currentVersionName, bumpType);
const nextTag = `android-v${nextVersionName}`;

console.log(`Bumping Android Client version:`);
console.log(`  Version Name: ${currentVersionName} -> ${nextVersionName} (${bumpType})`);
console.log(`  Version Code: ${currentVersionCode} -> ${nextVersionCode}`);
console.log(`  Target Tag:   ${nextTag}`);

// 2. Update gradle.properties
gradleProps = gradleProps
  .replace(/^VERSION_CODE=\d+/m, `VERSION_CODE=${nextVersionCode}`)
  .replace(/^VERSION_NAME=[^\r\n]+/m, `VERSION_NAME=${nextVersionName}`);

writeFileSync(gradlePropsPath, gradleProps, "utf-8");
console.log("Updated apps/android/gradle.properties");

// 3. Git commit and tag
try {
  const gitEnv = {
    ...process.env,
    GIT_CONFIG_COUNT: undefined,
    GIT_CONFIG_VALUE_0: undefined,
    GIT_CONFIG_VALUE_1: undefined,
  };
  delete gitEnv.GIT_CONFIG_COUNT;
  delete gitEnv.GIT_CONFIG_VALUE_0;
  delete gitEnv.GIT_CONFIG_VALUE_1;

  execSync(`git add "${gradlePropsPath}"`, {
    cwd: rootDir,
    env: gitEnv,
    stdio: "inherit",
  });

  execSync(`git commit -m "chore(release): bump android version to ${nextTag}"`, {
    cwd: rootDir,
    env: gitEnv,
    stdio: "inherit",
  });

  execSync(`git tag -a "${nextTag}" -m "Release ${nextTag}"`, {
    cwd: rootDir,
    env: gitEnv,
    stdio: "inherit",
  });

  console.log(`\nSuccessfully created tag ${nextTag}!`);
  console.log(`\nTo trigger GitHub Actions Android release build, run:`);
  console.log(`  git push origin HEAD --tags\n`);
} catch (error) {
  console.error("Git commit/tagging failed:", error);
  process.exit(1);
}
