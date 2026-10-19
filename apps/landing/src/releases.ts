export const packageIds = [
  "windows-exe",
  "windows-msi",
  "linux-deb",
  "linux-rpm",
  "linux-rpm-el10",
  "linux-appimage",
  "android-apk",
] as const;
export type PackageId = (typeof packageIds)[number];

export interface ReleaseAsset {
  name: string;
  browser_download_url: string;
}
export interface Release {
  tag_name: string;
  draft: boolean;
  prerelease: boolean;
  assets: ReleaseAsset[];
}
export interface Catalog {
  desktopVersion: string;
  androidVersion: string;
  desktopRelease: string;
  packages: Record<PackageId, string>;
}

function version(tag: string): number[] {
  return tag
    .replace(/^android-v|^v/, "")
    .split(".")
    .map(Number);
}
function newest(releases: Release[], pattern: RegExp): Release {
  const release = releases
    .filter((r) => !r.draft && !r.prerelease && pattern.test(r.tag_name))
    .sort((a, b) => {
      const av = version(a.tag_name),
        bv = version(b.tag_name);
      for (let i = 0; i < 3; i++) if (av[i] !== bv[i]) return bv[i]! - av[i]!;
      return 0;
    })[0];
  if (!release)
    throw new Error("No matching published Stable release was found");
  return release;
}
function asset(release: Release, name: string): string {
  const found = release.assets.find((a) => a.name === name);
  const expected = `https://github.com/ardli-firman/sha-print/releases/download/${release.tag_name}/${name}`;
  if (!found || found.browser_download_url !== expected)
    throw new Error(`Missing or unexpected release asset: ${name}`);
  return found.browser_download_url;
}
export function createCatalog(releases: Release[]): Catalog {
  const desktop = newest(releases, /^v\d+\.\d+\.\d+$/);
  const android = newest(releases, /^android-v\d+\.\d+\.\d+$/);
  const d = desktop.tag_name.slice(1),
    a = android.tag_name.slice("android-v".length);
  return {
    desktopVersion: d,
    androidVersion: a,
    desktopRelease: `https://github.com/ardli-firman/sha-print/releases/tag/${desktop.tag_name}`,
    packages: {
      "windows-exe": asset(desktop, `ShaPrint_${d}_x64-setup.exe`),
      "windows-msi": asset(desktop, `ShaPrint_${d}_x64_en-US.msi`),
      "linux-deb": asset(desktop, `shaprint_${d}-1_amd64.deb`),
      "linux-rpm": asset(desktop, `shaprint-${d}-1.x86_64.rpm`),
      "linux-rpm-el10": asset(desktop, `shaprint-${d}-1.el10.x86_64.rpm`),
      "linux-appimage": asset(desktop, `ShaPrint_${d}_amd64.AppImage`),
      "android-apk": asset(android, `shaprint-client-android-v${a}.apk`),
    },
  };
}
