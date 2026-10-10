import { describe, expect, it } from "bun:test";
import { updateProjectVersionAndEndpoint } from "./prepare-build";

describe("prepare-build version and endpoint injection", () => {
  const samplePkg = JSON.stringify({ name: "shaprint-desktop", version: "3.4.0" }, null, 2);
  const sampleTauri = JSON.stringify(
    {
      productName: "ShaPrint",
      version: "3.4.0",
      app: {
        windows: [
          {
            label: "main",
            title: "ShaPrint",
          },
        ],
      },
      plugins: {
        updater: {
          pubkey: "my-pubkey",
          endpoints: [
            "https://github.com/ardli-firman/sha-print/releases/latest/download/latest.json",
          ],
        },
      },
    },
    null,
    2,
  );
  const sampleCargo = `[package]
name = "shaprint-desktop"
version = "3.4.0"
edition = "2021"
`;

  it("updates versions across package.json, tauri.conf.json, and Cargo.toml", () => {
    const updated = updateProjectVersionAndEndpoint({
      pkgJsonContent: samplePkg,
      tauriConfContent: sampleTauri,
      cargoTomlContent: sampleCargo,
      version: "3.4.0-nightly.1",
      endpoint: "https://github.com/ardli-firman/sha-print/releases/download/nightly/latest.json",
    });

    const parsedPkg = JSON.parse(updated.pkgJsonContent);
    expect(parsedPkg.version).toBe("3.4.0-nightly.1");

    const parsedTauri = JSON.parse(updated.tauriConfContent);
    expect(parsedTauri.version).toBe("3.4.0-nightly.1");
    expect(parsedTauri.app.windows[0].title).toBe("ShaPrint (Nightly)");
    expect(parsedTauri.bundle.targets).toEqual(["nsis"]);
    expect(parsedTauri.plugins.updater.endpoints).toEqual([
      "https://github.com/ardli-firman/sha-print/releases/download/nightly/latest.json",
    ]);

    expect(updated.cargoTomlContent).toContain('version = "3.4.0-nightly.1"');
  });
});
