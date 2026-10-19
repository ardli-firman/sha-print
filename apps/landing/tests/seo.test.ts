import { describe, expect, test } from "bun:test";
import catalog from "../src/release-catalog.json";
import { renderSeo, renderSitemap, robotsTxt, seoPages, siteOrigin, structuredData } from "../src/seo";

describe("Search metadata", () => {
  test("keeps software versions and download targets tied to the same catalog as visible downloads", () => {
    const updated = {
      ...catalog, desktopVersion: "9.0.0", androidVersion: "2.0.0",
      packages: { ...catalog.packages, "android-apk": "https://github.com/ardli-firman/sha-print/releases/download/android-v2.0.0/shaprint-client-android-v2.0.0.apk" },
    };
    const graph = structuredData(seoPages[0], updated)["@graph"];
    expect(graph.find(node => node["@type"] === "SoftwareApplication")?.softwareVersion).toBe("9.0.0");
    const android = graph.find(node => node["@type"] === "MobileApplication");
    expect(android?.softwareVersion).toBe("2.0.0");
    expect(android?.downloadUrl).toBe(updated.packages["android-apk"]);
  });
  test("keeps embedded structured data inert if a future value contains closing script markup", () => {
    const dangerous = "</script><script>alert(1)</script>";
    const { head, json } = renderSeo(seoPages[0], { ...catalog, desktopVersion: dangerous });
    expect(json).not.toContain("<");
    expect(head.match(/<script\b/g)).toHaveLength(1);
    expect(head.match(/<\/script>/g)).toHaveLength(1);
    expect(JSON.parse(json)["@graph"].find((node: Record<string, unknown>) => node["@type"] === "SoftwareApplication").softwareVersion).toBe(dangerous);
  });
  test("does not invent offers, price, ratings, reviews, or search endpoints", () => {
    const text = JSON.stringify(structuredData(seoPages[0], catalog));
    for (const property of ["offers", "price", "review", "aggregateRating", "potentialAction"])
      expect(text).not.toContain(`"${property}"`);
  });
  test("maps secondary-page breadcrumbs to the same public canonical pages", () => {
    for (const page of seoPages.slice(1)) {
      const graph = structuredData(page, catalog)["@graph"];
      const trail = graph.find(node => node["@type"] === "BreadcrumbList")!;
      expect(trail.itemListElement).toEqual([
        { "@type": "ListItem", position: 1, name: "ShaPrint", item: siteOrigin + "/" },
        { "@type": "ListItem", position: 2, name: page.label, item: siteOrigin + page.path },
      ]);
    }
  });
  test("lists only canonical content pages for discovery and excludes API crawling", () => {
    const sitemap = renderSitemap();
    expect([...sitemap.matchAll(/<loc>(.*?)<\/loc>/g)].map(match => match[1])).toEqual(seoPages.map(page => siteOrigin + page.path));
    expect(sitemap).not.toContain("404");
    expect(sitemap).not.toContain("lastmod");
    expect(robotsTxt).toContain("Disallow: /api/");
    expect(robotsTxt).toContain(`Sitemap: ${siteOrigin}/sitemap.xml`);
  });
});
