import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { execSync } from "node:child_process";

const rootDir = resolve(import.meta.dirname, "..");
const pkgPath = resolve(rootDir, "apps/desktop/package.json");
const tauriConfPath = resolve(rootDir, "apps/desktop/src-tauri/tauri.conf.json");
const cargoTomlPath = resolve(rootDir, "apps/desktop/src-tauri/Cargo.toml");

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
  console.error("Usage: bun run scripts/release.ts [patch|minor|major]");
  process.exit(1);
}

// 1. Read current version
const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
const currentVersion = pkg.version;
const nextVersion = bumpVersion(currentVersion, bumpType);
const nextTag = `v${nextVersion}`;

console.log(`Bumping version: ${currentVersion} -> ${nextVersion} (${bumpType})`);

// 2. Update apps/desktop/package.json
pkg.version = nextVersion;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n", "utf-8");

// 3. Update apps/desktop/src-tauri/tauri.conf.json
const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf-8"));
tauriConf.version = nextVersion;
writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + "\n", "utf-8");

// 4. Update apps/desktop/src-tauri/Cargo.toml
let cargoToml = readFileSync(cargoTomlPath, "utf-8");
cargoToml = cargoToml.replace(
  /^version = "[^"]+"/m,
  `version = "${nextVersion}"`
);
writeFileSync(cargoTomlPath, cargoToml, "utf-8");

console.log("Updated version in package.json, tauri.conf.json, and Cargo.toml");

// 5. Git commit and tag
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

  execSync(`git add "${pkgPath}" "${tauriConfPath}" "${cargoTomlPath}"`, {
    cwd: rootDir,
    env: gitEnv,
    stdio: "inherit",
  });

  execSync(`git commit -m "chore(release): bump version to ${nextTag}"`, {
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
  console.log(`\nTo trigger GitHub Actions release build, run:`);
  console.log(`  git push origin HEAD --tags\n`);
} catch (error) {
  console.error("Git commit/tagging failed:", error);
  process.exit(1);
}
