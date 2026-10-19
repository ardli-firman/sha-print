import { createCatalog, type Catalog, type Release } from "./releases";

/** Read the complete bounded release history before choosing semantic versions. */
export async function fetchReleaseCatalog(
  fetcher: (url: string, init?: RequestInit) => Promise<Response> = fetch,
  token?: string,
): Promise<Catalog> {
  const releases: Release[] = [];
  for (let page = 1; page <= 10; page++) {
    const response = await fetcher(
      `https://api.github.com/repos/ardli-firman/sha-print/releases?per_page=100&page=${page}`,
      {
        headers: {
          Accept: "application/vnd.github+json",
          "User-Agent": "shaprint-landing-build",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        signal: AbortSignal.timeout(20000),
      },
    );
    if (!response.ok)
      throw new Error(`GitHub release lookup failed (${response.status}); catalog unchanged`);
    const batch: unknown = await response.json();
    if (!Array.isArray(batch))
      throw new Error("Unexpected GitHub release response; catalog unchanged");
    releases.push(...(batch as Release[]));
    if (batch.length < 100) return createCatalog(releases);
  }
  throw new Error("Release history exceeds 1,000 entries; catalog unchanged. Increase the reviewed history limit.");
}
