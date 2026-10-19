import { cp, mkdir, rm } from "node:fs/promises";
import catalog from "../src/release-catalog.json";
import { packageIds } from "../src/releases";
const root = new URL("../", import.meta.url);
const output = new URL("dist/", root);
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await cp(new URL("public/", root), output, { recursive: true });
// Only raw screenshots captured from the running Android app belong in this set.
for (const screen of ["discovery", "printers", "servers"])
  await cp(
    new URL(`../../img/android/android-${screen}.png`, root),
    new URL(`assets/android-${screen}.png`, output),
  );
const replacements: Record<string, string> = {
  ...catalog.packages,
  desktopVersion: catalog.desktopVersion,
  androidVersion: catalog.androidVersion,
  desktopRelease: catalog.desktopRelease,
  heroDiagramDesktop: await Bun.file(
    new URL("public/assets/network.svg", root),
  ).text(),
  heroDiagramMobile: await Bun.file(
    new URL("public/assets/network-mobile.svg", root),
  ).text(),
};
let html = await Bun.file(new URL("public/index.html", root)).text();
for (const [key, value] of Object.entries(replacements))
  html = html.replaceAll(`{{${key}}}`, value);
if (/\{\{[^}]+\}\}/.test(html))
  throw new Error("Unresolved template placeholder");
for (const id of packageIds)
  if (!html.includes(`data-download="${id}"`))
    throw new Error(`Missing download option ${id}`);
await Bun.write(new URL("index.html", output), html);
const result = await Bun.build({
  entrypoints: [new URL("src/site.ts", root).pathname],
  outdir: output.pathname,
  target: "browser",
  minify: true,
});
if (!result.success)
  throw new AggregateError(result.logs, "Landing browser build failed");
console.log(
  `Built landing, setup guide, privacy page and seven Stable package links into ${output.pathname}`,
);
