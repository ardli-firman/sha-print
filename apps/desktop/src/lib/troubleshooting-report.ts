import type { LocalTroubleshootingSnapshot } from "@/api/troubleshooting";

/** Public label maps shared by the dialog and the copyable session report. */
export const CHECK_LABELS: Record<string, string> = {
  spooler: "Native print spooler",
  "client-queue": "Client queue destination",
  "pending-jobs": "Pending print jobs",
  "client-proxy": "Client proxy",
  "server-reachability": "Server reachability",
  "server-identity": "Server identity",
  "shared-printer": "Shared printer availability",
  "network-channel": "Network Channel authorization",
  firewall: "Firewall inspection",
};

export const STATUS_LABELS: Record<string, string> = {
  passed: "Passed",
  failed: "Failed",
  inconclusive: "Inconclusive",
  "not-applicable": "Not applicable",
};

export const ACTION_LABELS: Record<string, string> = {
  "start-client-proxy": "Start client proxy",
  "repair-client-queue": "Repair Client queue",
  "send-test-print": "Send test print",
  "retry-test-print": "Retry test print",
};

export const OUTCOME_LABELS: Record<string, string> = {
  success: "Succeeded",
  denied: "Authorization denied",
  failed: "Failed",
  partial: "Partially completed",
  accepted: "Accepted by the native spooler",
  rejected: "Rejected before submission",
  uncertain: "Submission uncertain",
  blocked: "Blocked by fresh checks",
};

const label = (map: Record<string, string>, key: string) => map[key] ?? key;

/** Human label for a check/action/outcome id, falling back to the stable id itself. */
export const checkLabel = (id: string) => label(CHECK_LABELS, id);
export const checkStatusLabel = (status: string) => label(STATUS_LABELS, status);
export const actionLabel = (action: string) => label(ACTION_LABELS, action);
export const actionOutcomeLabel = (outcome: string) => label(OUTCOME_LABELS, outcome);

/**
 * Concise, session-only help-request report composed strictly from the bounded
 * public session snapshot. Check prose (`detail`) is deliberately excluded so
 * raw driver or system text can never reach the preview or the clipboard;
 * failures carry their stable error code instead.
 */
export function buildTroubleshootingReport(snapshot: LocalTroubleshootingSnapshot): string {
  const lines: string[] = [
    "ShaPrint troubleshooting report",
    `App version: ${snapshot.version}`,
    `Client queue: ${snapshot.target.queue_name}`,
    `Shared printer: ${snapshot.target.printer_name}`,
    `Server: ${snapshot.target.server_address}`,
  ];
  if (snapshot.requested_media) {
    lines.push(`Requested media: ${snapshot.requested_media}`);
  }
  if (snapshot.submission) {
    lines.push(`Submission: ${snapshot.submission}`);
  }
  if (snapshot.physical_result) {
    lines.push(`Physical result: ${snapshot.physical_result}`);
  }
  if (snapshot.user_symptom) {
    lines.push(`Observed symptom: ${snapshot.user_symptom}`);
  }
  lines.push("", "Checks:");
  if (snapshot.checks.length === 0) {
    lines.push("- none recorded");
  } else {
    for (const check of snapshot.checks) {
      const code = check.code ? ` [${check.code}]` : "";
      lines.push(`- ${checkLabel(check.check)}: ${checkStatusLabel(check.status)}${code}`);
    }
  }
  lines.push("", "Attempted actions:");
  if (snapshot.attempted_actions.length === 0) {
    lines.push("- none recorded");
  } else {
    for (const action of snapshot.attempted_actions) {
      lines.push(`- ${actionLabel(action.action)}: ${actionOutcomeLabel(action.outcome)}`);
    }
  }
  lines.push("", "Session-only report. ShaPrint keeps no troubleshooting history.");
  return lines.join("\n");
}
