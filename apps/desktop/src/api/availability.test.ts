import { describe, expect, it } from "vitest";

import { printRecovery } from "./availability";
import type { ServiceState, ServiceStatus } from "./types";

function service(id: ServiceStatus["id"], state: ServiceState): ServiceStatus {
  return { id, state, detail: "" };
}

describe("print availability", () => {
  it("says what to start when the always-on print path is stopped", () => {
    expect(printRecovery(service("client-proxy", "stopped"))).toBe(
      "Start the client proxy to print from queues installed on this computer.",
    );
  });

  it("does not call a stopped sharing service a problem", () => {
    // Sharing is off until the user starts it, so a stopped sharing service is the default state,
    // not a print failure to act on.
    expect(printRecovery(service("server-sharing", "stopped"))).toBeNull();
  });

  it("says nothing while a print path is running", () => {
    expect(printRecovery(service("client-proxy", "running"))).toBeNull();
    expect(printRecovery(service("server-sharing", "running"))).toBeNull();
  });

  it("reports a print path that failed as retryable", () => {
    expect(printRecovery(service("client-proxy", "failed"))).toContain("use Start to try again");
    expect(printRecovery(service("server-sharing", "failed"))).toContain(
      "Other ShaPrint users cannot print",
    );
  });

  it("calls out a print path that is still moving through its lifecycle", () => {
    expect(printRecovery(service("client-proxy", "starting"))).toContain("unavailable");
    expect(printRecovery(service("server-sharing", "stopping"))).toContain("cannot print");
  });

  it("ignores services that no print depends on", () => {
    expect(printRecovery(service("server-discovery", "stopped"))).toBeNull();
    expect(printRecovery(service("server-discovery", "failed"))).toBeNull();
  });
});
