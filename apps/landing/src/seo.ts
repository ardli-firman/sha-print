import type { Catalog } from "./releases";

export const siteOrigin = "https://sha-print.pages.dev";
export const seoPages = [
  {
    path: "/", file: "index.html", label: "ShaPrint",
    title: "ShaPrint | USB Printer Sharing for Windows, Linux & Android",
    description: "Share an existing USB printer across your network with ShaPrint for Windows, Linux, and Android. No printer Wi-Fi needed. Your server stays on while you print.",
  },
  {
    path: "/guide/", file: "guide/index.html", label: "Setup guide",
    title: "ShaPrint Setup Guide | Share a USB Printer over LAN",
    description: "Set up a ShaPrint server, share a USB printer, and connect Windows, Linux, or Android clients. Check drivers, Linux packages, and cross-VLAN requirements.",
  },
  {
    path: "/privacy/", file: "privacy/index.html", label: "Privacy policy",
    title: "ShaPrint Privacy Policy | Android App & Website",
    description: "Read how ShaPrint handles documents, network printing, local storage, Android backup, and website measurements, plus data deletion and privacy contact details.",
  },
] as const;
export type SeoPage = typeof seoPages[number];

const html = (value: string) => value.replace(/[&<>"']/g, character => ({
  "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
})[character]!);

export function structuredData(page: SeoPage, catalog: Catalog) {
  const url = siteOrigin + page.path;
  const graph: Record<string, unknown>[] = [{
    "@type": "WebSite", "@id": siteOrigin + "/#website",
    name: "ShaPrint", url: siteOrigin + "/", inLanguage: "en",
  }, {
    "@type": "WebPage", "@id": url + "#webpage", url,
    name: page.title, description: page.description, inLanguage: "en",
    isPartOf: { "@id": siteOrigin + "/#website" },
    ...(page.path === "/" ? {
      about: [{ "@id": siteOrigin + "/#desktop-app" }, { "@id": siteOrigin + "/#android-app" }],
    } : { breadcrumb: { "@id": url + "#breadcrumb" } }),
  }];
  if (page.path === "/") {
    graph.push({
      "@type": "SoftwareApplication", "@id": siteOrigin + "/#desktop-app",
      name: "ShaPrint", url, applicationCategory: "UtilitiesApplication",
      operatingSystem: "Windows 10 or later, Linux", softwareVersion: catalog.desktopVersion,
      description: "Share an existing USB printer through a Windows or Linux server with desktop and Android clients. The server needs a working printer driver and stays on while clients print.",
      screenshot: siteOrigin + "/assets/desktop-preview.png",
      downloadUrl: [catalog.packages["windows-exe"], catalog.packages["linux-deb"], catalog.packages["linux-rpm"], catalog.packages["linux-rpm-el10"], catalog.packages["linux-appimage"], catalog.packages["windows-msi"]],
      installUrl: siteOrigin + "/guide/",
    }, {
      "@type": "MobileApplication", "@id": siteOrigin + "/#android-app",
      name: "ShaPrint for Android", url: siteOrigin + "/#android-title",
      applicationCategory: "UtilitiesApplication", operatingSystem: "Android 8.0 or later",
      softwareVersion: catalog.androidVersion, downloadUrl: catalog.packages["android-apk"],
      description: "An Android client that sends print jobs directly to a trusted ShaPrint server's shared printers.",
      screenshot: siteOrigin + "/assets/android-discovery.png",
      installUrl: siteOrigin + "/guide/#android",
    });
  } else {
    graph.push({
      "@type": "BreadcrumbList", "@id": url + "#breadcrumb",
      itemListElement: [
        { "@type": "ListItem", position: 1, name: "ShaPrint", item: siteOrigin + "/" },
        { "@type": "ListItem", position: 2, name: page.label, item: url },
      ],
    });
  }
  // No offers, pricing, reviews, or ratings are asserted without product evidence.
  return { "@context": "https://schema.org", "@graph": graph };
}

export function renderSeo(page: SeoPage, catalog: Catalog) {
  const url = siteOrigin + page.path;
  const image = siteOrigin + "/assets/social.png";
  const imageAlt = "ShaPrint shares an existing USB printer through a server computer with clients on the network.";
  // JSON stays a data block, even if a future catalog/string contains HTML delimiters.
  const json = JSON.stringify(structuredData(page, catalog)).replace(/</g, "\\u003c");
  const meta = (key: string, value: string, property = false) =>
    `<meta ${property ? "property" : "name"}="${key}" content="${html(value)}" />`;
  return {
    json,
    head: [
      `<title>${html(page.title)}</title>`,
      meta("description", page.description),
      meta("application-name", "ShaPrint"),
      meta("robots", "index, follow, max-image-preview:large"),
      `<link rel="canonical" href="${html(url)}" />`,
      ...Object.entries({
        "og:type": "website", "og:site_name": "ShaPrint", "og:locale": "en_US",
        "og:title": page.title, "og:description": page.description, "og:url": url,
        "og:image": image, "og:image:type": "image/png", "og:image:width": "1200",
        "og:image:height": "630", "og:image:alt": imageAlt,
      }).map(([key, value]) => meta(key, value, true)),
      ...Object.entries({
        "twitter:card": "summary_large_image", "twitter:title": page.title,
        "twitter:description": page.description, "twitter:image": image, "twitter:image:alt": imageAlt,
      }).map(([key, value]) => meta(key, value)),
      `<script type="application/ld+json">${json}</script>`,
    ].join("\n    "),
  };
}

export function renderSitemap() {
  return '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n' +
    seoPages.map(page => `  <url><loc>${siteOrigin}${page.path}</loc></url>`).join("\n") +
    "\n</urlset>\n";
}
export const robotsTxt = `User-agent: *\nAllow: /\nDisallow: /api/\nSitemap: ${siteOrigin}/sitemap.xml\n`;
