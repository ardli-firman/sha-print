import catalog from "../src/release-catalog.json";
import { packageIds } from "../src/releases";

// Public HEAD requests follow GitHub's asset redirects without downloading installers.
// Do not send the GitHub API token to the public download servers.
const results = await Promise.allSettled(packageIds.map(async (id) => {
  for (let attempt = 1; attempt <= 3; attempt++) {
    try {
      const response = await fetch(catalog.packages[id], {
        method: "HEAD",
        signal: AbortSignal.timeout(20000),
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      console.log(`${id}: available (${response.status})`);
      return;
    } catch {
      if (attempt === 3) throw new Error(`Release download unavailable: ${id}`);
      await Bun.sleep(2000 * attempt);
    }
  }
}));
if (results.some(result => result.status === "rejected")) {
  for (const result of results)
    if (result.status === "rejected") console.error(result.reason.message);
  process.exitCode = 1;
} else console.log("All seven release downloads are available");
