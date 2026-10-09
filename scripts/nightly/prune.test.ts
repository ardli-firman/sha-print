import { describe, expect, it } from "bun:test";
import { determinePrunePlan, type GitHubReleaseItem } from "./prune";

describe("nightly release retention and pruning", () => {
  const feedHolder: GitHubReleaseItem = {
    tagName: "nightly",
    name: "Nightly Update Feed",
    isPrerelease: true,
  };

  const stableReleases: GitHubReleaseItem[] = [
    { tagName: "v3.4.0", name: "ShaPrint v3.4.0", isPrerelease: false },
    { tagName: "v3.3.1", name: "ShaPrint v3.3.1", isPrerelease: false },
    { tagName: "v1.6.3-stable", name: "ShaPrint v1.6.3", isPrerelease: false },
  ];

  it("retains all nightly releases when count is 10 or fewer", () => {
    const nightlies: GitHubReleaseItem[] = Array.from({ length: 8 }, (_, i) => ({
      tagName: `v3.4.0-nightly.${i + 1}`,
      name: `ShaPrint 3.4.0-nightly.${i + 1}`,
      isPrerelease: true,
    }));

    const plan = determinePrunePlan({
      releases: [feedHolder, ...stableReleases, ...nightlies],
      activeFeedTag: "v3.4.0-nightly.8",
      retainCount: 10,
    });

    expect(plan.toRetain.map((r) => r.tagName)).toEqual(
      nightlies.map((r) => r.tagName).reverse(),
    );
    expect(plan.toDelete).toHaveLength(0);
    expect(plan.stableReleases).toHaveLength(3);
    expect(plan.feedHolder?.tagName).toBe("nightly");
  });

  it("keeps exactly the 10 newest versioned nightlies and marks older for deletion", () => {
    // 15 nightly releases from 1 to 15
    const nightlies: GitHubReleaseItem[] = Array.from({ length: 15 }, (_, i) => ({
      tagName: `v3.4.0-nightly.${i + 1}`,
      name: `ShaPrint 3.4.0-nightly.${i + 1}`,
      isPrerelease: true,
    }));

    const plan = determinePrunePlan({
      releases: [feedHolder, ...stableReleases, ...nightlies],
      activeFeedTag: "v3.4.0-nightly.15",
      retainCount: 10,
    });

    // 10 newest: 15 down to 6
    expect(plan.toRetain).toHaveLength(10);
    expect(plan.toRetain.map((r) => r.tagName)).toEqual([
      "v3.4.0-nightly.15",
      "v3.4.0-nightly.14",
      "v3.4.0-nightly.13",
      "v3.4.0-nightly.12",
      "v3.4.0-nightly.11",
      "v3.4.0-nightly.10",
      "v3.4.0-nightly.9",
      "v3.4.0-nightly.8",
      "v3.4.0-nightly.7",
      "v3.4.0-nightly.6",
    ]);

    // 5 older: 5 down to 1 marked for deletion
    expect(plan.toDelete).toHaveLength(5);
    expect(plan.toDelete.map((r) => r.tagName)).toEqual([
      "v3.4.0-nightly.5",
      "v3.4.0-nightly.4",
      "v3.4.0-nightly.3",
      "v3.4.0-nightly.2",
      "v3.4.0-nightly.1",
    ]);
  });

  it("never selects feed holder or stable releases for deletion", () => {
    const nightlies: GitHubReleaseItem[] = Array.from({ length: 12 }, (_, i) => ({
      tagName: `v3.4.0-nightly.${i + 1}`,
      name: `ShaPrint 3.4.0-nightly.${i + 1}`,
      isPrerelease: true,
    }));

    const plan = determinePrunePlan({
      releases: [feedHolder, ...stableReleases, ...nightlies],
      activeFeedTag: "v3.4.0-nightly.12",
      retainCount: 10,
    });

    const deletedTags = plan.toDelete.map((r) => r.tagName);
    expect(deletedTags).not.toContain("nightly");
    expect(deletedTags).not.toContain("v3.4.0");
    expect(deletedTags).not.toContain("v3.3.1");
    expect(deletedTags).not.toContain("v1.6.3-stable");
  });

  it("protects active feed release even if outside top 10", () => {
    const nightlies: GitHubReleaseItem[] = Array.from({ length: 15 }, (_, i) => ({
      tagName: `v3.4.0-nightly.${i + 1}`,
      name: `ShaPrint 3.4.0-nightly.${i + 1}`,
      isPrerelease: true,
    }));

    // Suppose active feed is v3.4.0-nightly.2
    const plan = determinePrunePlan({
      releases: [feedHolder, ...stableReleases, ...nightlies],
      activeFeedTag: "v3.4.0-nightly.2",
      retainCount: 10,
    });

    const retainedTags = plan.toRetain.map((r) => r.tagName);
    expect(retainedTags).toContain("v3.4.0-nightly.2");
    const deletedTags = plan.toDelete.map((r) => r.tagName);
    expect(deletedTags).not.toContain("v3.4.0-nightly.2");
  });
});
