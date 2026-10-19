import { fetchReleaseCatalog } from "../src/release-refresh";

// Write only after the complete catalog passes validation. CI supplies a read-only token.
const catalog = await fetchReleaseCatalog(fetch, process.env.GITHUB_TOKEN);
await Bun.write(
  new URL("../src/release-catalog.json", import.meta.url),
  JSON.stringify(catalog, null, 2) + "\n",
);
console.log(
  `Verified desktop Stable ${catalog.desktopVersion} and Android ${catalog.androidVersion}; refreshed all seven package links`,
);
