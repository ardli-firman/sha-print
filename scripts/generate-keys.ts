import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { execSync } from "node:child_process";

const rootDir = resolve(import.meta.dirname, "..");
const keysDir = resolve(rootDir, ".keys");
const privateKeyPath = resolve(keysDir, "shaprint.key");
const publicKeyPath = resolve(keysDir, "shaprint.key.pub");

if (!existsSync(keysDir)) {
  mkdirSync(keysDir, { recursive: true });
}

if (existsSync(privateKeyPath) && !process.argv.includes("--force")) {
  console.log(`\nKeys already exist at:\n  Private: ${privateKeyPath}\n  Public:  ${publicKeyPath}`);
  console.log(`Use --force to overwrite.\n`);
} else {
  console.log("Generating Minisign key pair for Tauri updater...");
  try {
    execSync(
      `bun run tauri signer generate --ci -p "" -w "${privateKeyPath}" --force`,
      {
        cwd: resolve(rootDir, "apps/desktop"),
        stdio: "inherit",
      }
    );
  } catch (err) {
    console.error("Failed to generate keys:", err);
    process.exit(1);
  }
}

if (existsSync(publicKeyPath) && existsSync(privateKeyPath)) {
  const pubContent = readFileSync(publicKeyPath, "utf-8").trim();
  const privContent = readFileSync(privateKeyPath, "utf-8").trim();

  // Automatically sync public key to tauri.conf.json
  const tauriConfPath = resolve(rootDir, "apps/desktop/src-tauri/tauri.conf.json");
  if (existsSync(tauriConfPath)) {
    const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf-8"));
    if (tauriConf.plugins?.updater) {
      tauriConf.plugins.updater.pubkey = pubContent;
      writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + "\n", "utf-8");
      console.log("Updated public key in apps/desktop/src-tauri/tauri.conf.json");
    }
  }

  console.log("\n========================================================");
  console.log("               TAURI SIGNING KEYS GENERATED             ");
  console.log("========================================================");
  console.log(`\n1. Public Key (stored in apps/desktop/src-tauri/tauri.conf.json):`);
  console.log(`   ${pubContent}\n`);
  console.log(`2. GitHub Secret Setup:`);
  console.log(`   Go to GitHub Repository -> Settings -> Secrets and variables -> Actions`);
  console.log(`   (Or under Environment: 'release' -> Environment secrets)`);
  console.log(`\n   Add Secret:`);
  console.log(`   - Name:  TAURI_SIGNING_PRIVATE_KEY`);
  console.log(`   - Value:\n${privContent}\n`);
  console.log(`   (Optional password if set, otherwise leave empty):`);
  console.log(`   - Name:  TAURI_SIGNING_PRIVATE_KEY_PASSWORD`);
  console.log(`   - Value: (empty)\n`);
  console.log("========================================================\n");
}
