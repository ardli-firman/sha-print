import { describe, expect, test } from "bun:test";
import { fetchReleaseCatalog } from "../src/release-refresh";
import { createCatalog, type Release } from "../src/releases";
const base = "https://github.com/ardli-firman/sha-print/releases/download/";
function release(tag: string): Release {
  const v = tag.replace(/^android-v|^v/, "");
  const names = tag.startsWith("android-")
    ? [`shaprint-client-android-v${v}.apk`]
    : [
        `ShaPrint_${v}_x64-setup.exe`,
        `ShaPrint_${v}_x64_en-US.msi`,
        `shaprint_${v}-1_amd64.deb`,
        `shaprint-${v}-1.x86_64.rpm`,
        `shaprint-${v}-1.el10.x86_64.rpm`,
        `ShaPrint_${v}_amd64.AppImage`,
      ];
  return {
    tag_name: tag,
    draft: false,
    prerelease: false,
    assets: names.map((name) => ({
      name,
      browser_download_url: `${base}${tag}/${name}`,
    })),
  };
}
describe("Stable package selection", () => {
  test("keeps newer Android and Nightly releases out of desktop links", () => {
    const nightly = { ...release("v8.0.0"), prerelease: true };
    const catalog = createCatalog([
      release("android-v10.0.0"),
      nightly,
      release("v3.4.4"),
      release("v3.4.3"),
    ]);
    expect(catalog.desktopVersion).toBe("3.4.4");
    expect(catalog.androidVersion).toBe("10.0.0");
    expect(catalog.packages["linux-deb"]).toContain(
      "/v3.4.4/shaprint_3.4.4-1_amd64.deb",
    );
  });
  test("compares semantic version numbers rather than strings", () => {
    expect(
      createCatalog([
        release("v3.9.0"),
        release("v3.10.0"),
        release("android-v1.0.0"),
      ]).desktopVersion,
    ).toBe("3.10.0");
  });
  test("excludes draft and suffixed releases even if mislabelled", () => {
    const draft = { ...release("v9.0.0"), draft: true };
    expect(
      createCatalog([
        draft,
        release("v8.0.0-nightly.1"),
        release("v3.4.4"),
        release("android-v1.0.0"),
      ]).desktopVersion,
    ).toBe("3.4.4");
  });
  test("fails rather than publishing a partially missing download catalog", () => {
    const desktop = release("v3.4.4");
    desktop.assets.pop();
    expect(() => createCatalog([desktop, release("android-v1.0.0")])).toThrow(
      "Missing or unexpected release asset",
    );
  });
  test("rejects an asset URL from a different repository or host", () => {
    const desktop = release("v3.4.4");
    desktop.assets[0]!.browser_download_url = "https://example.com/package.exe";
    expect(() => createCatalog([desktop, release("android-v1.0.0")])).toThrow();
  });
  test("does not substitute Android when no desktop Stable exists", () => {
    expect(() => createCatalog([release("android-v1.0.0")])).toThrow(
      "No matching published Stable",
    );
  });
});

describe("Release refresh", () => {
  test("looks beyond the first page before choosing the newest semantic version", async () => {
    const pages = [
      [release("v3.4.4"), release("android-v1.0.0"), ...Array.from({ length: 98 }, () => ({ ...release("v8.0.0"), prerelease: true }))],
      [release("v3.10.0")],
    ];
    const urls: string[] = [];
    const fetcher = (async (url: string | URL | Request) => {
      urls.push(String(url));
      return Response.json(pages.shift());
    });
    const result = await fetchReleaseCatalog(fetcher);
    expect(result.desktopVersion).toBe("3.10.0");
    expect(result.androidVersion).toBe("1.0.0");
    expect(urls).toEqual([
      "https://api.github.com/repos/ardli-firman/sha-print/releases?per_page=100&page=1",
      "https://api.github.com/repos/ardli-firman/sha-print/releases?per_page=100&page=2",
    ]);
  });
  test("fails on a later API error rather than returning an earlier catalog", async () => {
    let calls = 0;
    const fetcher = (async () => ++calls === 1
      ? Response.json(Array.from({ length: 100 }, () => release("v3.4.4")))
      : new Response(null, { status: 503 }));
    await expect(fetchReleaseCatalog(fetcher)).rejects.toThrow("GitHub release lookup failed (503)");
  });
  test("rejects an invalid API response", async () => {
    const fetcher = (async () => Response.json({ message: "unexpected" }));
    await expect(fetchReleaseCatalog(fetcher)).rejects.toThrow("Unexpected GitHub release response");
  });
  test("does not fall back to an older version when the newest release is incomplete", async () => {
    const incomplete = release("v4.0.0");
    incomplete.assets.pop();
    const fetcher = (async () => Response.json([
      incomplete, release("v3.4.4"), release("android-v1.0.0"),
    ]));
    await expect(fetchReleaseCatalog(fetcher)).rejects.toThrow("Missing or unexpected release asset");
  });
  test("fails closed if the complete release history exceeds the pagination limit", async () => {
    let calls = 0;
    const fetcher = (async () => {
      calls++;
      return Response.json(Array.from({ length: 100 }, () => release("v3.4.4")));
    });
    await expect(fetchReleaseCatalog(fetcher)).rejects.toThrow("exceeds 1,000 entries");
    expect(calls).toBe(10);
  });
});
