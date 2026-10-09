import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

export interface UpdateFilesOptions {
  pkgJsonContent: string;
  tauriConfContent: string;
  cargoTomlContent: string;
  version: string;
  endpoint?: string;
}

export interface UpdateFilesResult {
  pkgJsonContent: string;
  tauriConfContent: string;
  cargoTomlContent: string;
}

/**
 * Pure function to inject nightly version and updater endpoint into config files.
 */
export function updateProjectVersionAndEndpoint(
  options: UpdateFilesOptions,
): UpdateFilesResult {
  const version = options.version.replace(/^v/, "");

  // Update package.json
  const pkg = JSON.parse(options.pkgJsonContent);
  pkg.version = version;
  const nextPkgJson = JSON.stringify(pkg, null, 2) + "\n";

  // Update tauri.conf.json
  const tauri = JSON.parse(options.tauriConfContent);
  tauri.version = version;
  if (version.includes("-nightly")) {
    if (tauri.app?.windows?.[0]) {
      tauri.app.windows[0].title = "ShaPrint (Nightly)";
    }
  }
  if (options.endpoint) {
    if (!tauri.plugins) tauri.plugins = {};
    if (!tauri.plugins.updater) tauri.plugins.updater = {};
    tauri.plugins.updater.endpoints = [options.endpoint];
  }
  const nextTauriConf = JSON.stringify(tauri, null, 2) + "\n";

  // Update Cargo.toml
  const nextCargoToml = options.cargoTomlContent.replace(
    /^version = "[^"]+"/m,
    `version = "${version}"`,
  );

  return {
    pkgJsonContent: nextPkgJson,
    tauriConfContent: nextTauriConf,
    cargoTomlContent: nextCargoToml,
  };
}

/**
 * Applies updates to disk in the given root directory.
 */
export function applyProjectVersionAndEndpoint(options: {
  rootDir: string;
  version: string;
  endpoint?: string;
}): void {
  const rootDir = options.rootDir;
  const pkgPath = resolve(rootDir, "apps/desktop/package.json");
  const tauriConfPath = resolve(rootDir, "apps/desktop/src-tauri/tauri.conf.json");
  const cargoTomlPath = resolve(rootDir, "apps/desktop/src-tauri/Cargo.toml");

  const pkgJsonContent = readFileSync(pkgPath, "utf-8");
  const tauriConfContent = readFileSync(tauriConfPath, "utf-8");
  const cargoTomlContent = readFileSync(cargoTomlPath, "utf-8");

  const updated = updateProjectVersionAndEndpoint({
    pkgJsonContent,
    tauriConfContent,
    cargoTomlContent,
    version: options.version,
    endpoint: options.endpoint,
  });

  writeFileSync(pkgPath, updated.pkgJsonContent, "utf-8");
  writeFileSync(tauriConfPath, updated.tauriConfContent, "utf-8");
  writeFileSync(cargoTomlPath, updated.cargoTomlContent, "utf-8");
}
