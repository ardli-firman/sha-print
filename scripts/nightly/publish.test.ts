import { describe, expect, it } from "bun:test";
import { executePrunePlan, type ReleaseClient } from "./publish";
import type { PrunePlan } from "./prune";

describe("prune automation execution", () => {
  it("executes release and tag deletion for planned items only", async () => {
    const deletedReleases: string[] = [];

    const mockClient: ReleaseClient = {
      listReleases: async () => [],
      getFeedMetadata: async () => null,
      uploadFeedAsset: async () => {},
      deleteReleaseAndTag: async (tag: string) => {
        deletedReleases.push(tag);
      },
    };

    const plan: PrunePlan = {
      toRetain: [
        { tagName: "v3.4.0-nightly.3", isPrerelease: true },
        { tagName: "v3.4.0-nightly.2", isPrerelease: true },
      ],
      toDelete: [
        { tagName: "v3.4.0-nightly.1", isPrerelease: true },
        { tagName: "v3.4.0-nightly.0", isPrerelease: true },
      ],
      feedHolder: { tagName: "nightly", isPrerelease: true },
      stableReleases: [{ tagName: "v3.4.0", isPrerelease: false }],
    };

    const count = await executePrunePlan(plan, mockClient);
    expect(count).toBe(2);
    expect(deletedReleases).toEqual(["v3.4.0-nightly.1", "v3.4.0-nightly.0"]);
  });

  it("never deletes feed holder or stable releases even if erroneously passed in toDelete", async () => {
    const deletedReleases: string[] = [];

    const mockClient: ReleaseClient = {
      listReleases: async () => [],
      getFeedMetadata: async () => null,
      uploadFeedAsset: async () => {},
      deleteReleaseAndTag: async (tag: string) => {
        deletedReleases.push(tag);
      },
    };

    const plan: PrunePlan = {
      toRetain: [],
      toDelete: [
        { tagName: "nightly", isPrerelease: true },
        { tagName: "v3.4.0", isPrerelease: false },
        { tagName: "v3.4.0-nightly.1", isPrerelease: true },
      ],
      stableReleases: [],
    };

    const count = await executePrunePlan(plan, mockClient);
    expect(count).toBe(1);
    expect(deletedReleases).toEqual(["v3.4.0-nightly.1"]);
  });
});
