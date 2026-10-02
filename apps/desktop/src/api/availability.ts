/**
 * What a user can do about a supervised service that is not currently available for printing.
 *
 * Availability is derived from the live runtime status the shell already publishes, so the window
 * can say why a print cannot proceed without waiting for a failed attempt: a stopped client proxy
 * means the queue's request never reaches this app at all.
 */

import type { ServiceId, ServiceStatus } from "./types";

/** How one print path is reported while it is not doing its job. */
interface PrintPath {
  /** Shown while the service is starting or stopping. */
  moving: string;
  /** Shown when the service failed to start, or stopped unexpectedly. */
  failed: string;
  /**
   * Shown when the service is stopped and starting it is the user's move. Absent when a stopped
   * service is the documented default rather than a problem.
   */
  stopped?: string;
}

const PRINT_PATHS: Partial<Record<ServiceId, PrintPath>> = {
  "client-proxy": {
    moving: "Printing from queues installed on this computer is unavailable while the client proxy changes state.",
    failed:
      "Printing from queues installed on this computer is unavailable. The client proxy failed to start or stopped unexpectedly; use Start to try again.",
    stopped: "Start the client proxy to print from queues installed on this computer.",
  },
  "server-sharing": {
    moving: "Other ShaPrint users cannot print to your queues while server sharing changes state.",
    // Sharing is off until the user starts it, so a stopped sharing service is not a problem to
    // report; only a failure is.
    failed:
      "Other ShaPrint users cannot print to your selected queues. Server sharing failed to start or stopped unexpectedly; use Start to try again.",
  },
};

/**
 * The action that restores printing through `service`.
 *
 * Returns `null` when the service is running, when a stopped service is the user's own choice or
 * the documented default, and when the service is not part of a print path at all (for example
 * server discovery, which only feeds the nearby-server list).
 */
export function printRecovery(service: ServiceStatus): string | null {
  const path = PRINT_PATHS[service.id];
  if (!path || service.state === "running") {
    return null;
  }
  if (service.state === "failed") {
    return path.failed;
  }
  if (service.state === "stopped") {
    return path.stopped ?? null;
  }
  return path.moving;
}
