import { useEffect, useRef, useState } from "react";
import { startService } from "@/api/ipc";
import { repairPrinterQueue } from "@/api/serverConnections";
import { inspectLocalClientQueue, type LocalTroubleshootingSnapshot } from "@/api/troubleshooting";
import type { RecognisedClientQueue } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Dialog, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { CLIENT_VERSION } from "@/lib/version";

const CHECK_LABELS: Record<string, string> = {
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

const STATUS_LABELS: Record<string, string> = {
  passed: "Passed", failed: "Failed", inconclusive: "Inconclusive", "not-applicable": "Not applicable",
};

const ACTION_LABELS: Record<string, string> = {
  "start-client-proxy": "Start client proxy",
  "repair-client-queue": "Repair Client queue",
};

const OUTCOME_LABELS: Record<string, string> = {
  success: "Succeeded",
  denied: "Authorization denied",
  failed: "Failed",
  partial: "Partially completed",
};

function classifyActionOutcome(error: unknown): "denied" | "failed" | "partial" {
  const errObj = error && typeof error === "object" ? (error as { code?: string; message?: string }) : null;
  const code = errObj?.code?.toLowerCase() ?? "";
  const msg = (typeof error === "string" ? error : errObj?.message ?? "").toLowerCase();
  if (code === "partial" || msg.includes("partial")) {
    return "partial";
  }
  if (
    code === "not-authorized" ||
    msg.includes("permission was not granted") ||
    msg.includes("administrator permission") ||
    msg.includes("authentication was not granted") ||
    msg.includes("authentication was dismissed") ||
    msg.includes("not authorized") ||
    msg.includes("denied")
  ) {
    return "denied";
  }
  return "failed";
}

export function LocalTroubleshootingDialog({ target, onClose, onReviewServer, onConfigureNetworkChannel }: {
  target: RecognisedClientQueue;
  onClose: () => void;
  onReviewServer?: (serverAddress: string) => void;
  onConfigureNetworkChannel?: () => void;
}) {
  const [run, setRun] = useState(0);
  const [snapshot, setSnapshot] = useState<LocalTroubleshootingSnapshot | null>(null);
  const [error, setError] = useState(false);
  const [isExecutingAction, setIsExecutingAction] = useState(false);
  const attemptedActionsRef = useRef<{ action: string; outcome: string }[]>([]);
  const activeRef = useRef(true);

  useEffect(() => {
    activeRef.current = true;
    let active = true;
    let settled = false;
    setSnapshot(null);
    setError(false);
    const timer = window.setTimeout(() => {
      if (!active || settled) return;
      settled = true;
      setSnapshot({
        target, version: CLIENT_VERSION, queue_empty: false,
        test_print_enabled: false, repair_enabled: false,
        attempted_actions: [...attemptedActionsRef.current],
        checks: Object.keys(CHECK_LABELS).map((check) => ({
          check, status: "inconclusive", code: "timeout",
          detail: "This check could not be completed within 15 seconds. Its result is unknown.",
        })),
      });
    }, 15_000);
    void inspectLocalClientQueue(target).then(
      (result) => {
        if (!active || settled) return;
        settled = true;
        window.clearTimeout(timer);
        setSnapshot({
          ...result,
          attempted_actions: [...attemptedActionsRef.current],
        });
      },
      () => {
        if (!active || settled) return;
        settled = true;
        window.clearTimeout(timer);
        setError(true);
      },
    );
    return () => {
      active = false;
      activeRef.current = false;
      window.clearTimeout(timer);
    };
  }, [target, run]);

  const handleStartProxy = async () => {
    if (isExecutingAction) return;
    setIsExecutingAction(true);
    let outcome: "success" | "denied" | "failed" | "partial" = "success";
    try {
      await startService("client-proxy");
    } catch (err) {
      outcome = classifyActionOutcome(err);
    }
    if (!activeRef.current) return;
    attemptedActionsRef.current = [
      ...attemptedActionsRef.current,
      { action: "start-client-proxy", outcome },
    ];
    setIsExecutingAction(false);
    setRun((v) => v + 1);
  };

  const handleRepairQueue = async () => {
    if (isExecutingAction) return;
    setIsExecutingAction(true);
    let outcome: "success" | "denied" | "failed" | "partial" = "success";
    try {
      await repairPrinterQueue(target.server_address, target.printer_name);
    } catch (err) {
      outcome = classifyActionOutcome(err);
    }
    if (!activeRef.current) return;
    attemptedActionsRef.current = [
      ...attemptedActionsRef.current,
      { action: "repair-client-queue", outcome },
    ];
    setIsExecutingAction(false);
    setRun((v) => v + 1);
  };

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}
      ariaLabelledBy="troubleshoot-title" ariaDescribedBy="troubleshoot-description">
      <DialogHeader>
        <DialogTitle id="troubleshoot-title">Troubleshoot printer</DialogTitle>
        <DialogDescription id="troubleshoot-description">
          Check this Client queue without printing or changing your configuration.
        </DialogDescription>
      </DialogHeader>
      <dl className="text-sm space-y-1 mb-4">
        <div><dt className="inline font-semibold">Client queue: </dt><dd className="inline">{target.queue_name}</dd></div>
        <div><dt className="inline font-semibold">Shared printer: </dt><dd className="inline">{target.printer_name}</dd></div>
        <div><dt className="inline font-semibold">Server: </dt><dd className="inline">{target.server_address}</dd></div>
      </dl>
      <div aria-live="polite">
        {!snapshot && !error ? <p role="status">Checking the local print path. Available results appear within 15 seconds.</p> : null}
        {error ? <p role="alert">Local checks could not be completed. The queue state is unknown, so testing and repair remain blocked. Check ShaPrint and try again.</p> : null}
        {snapshot ? <>
          <ul className="space-y-3">
            {snapshot.checks.map((check) => (
              <li key={check.check} className="rounded-md border p-3 text-sm">
                <p className="font-semibold">
                  {CHECK_LABELS[check.check] ?? check.check}{" "}
                  <span className="font-normal">
                    ({STATUS_LABELS[check.status] ?? "Inconclusive"})
                  </span>
                </p>
                <p>{check.detail}</p>
                {check.check === "client-proxy" && check.status === "failed" ? (
                  <div className="mt-2">
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="text-xs h-7"
                      disabled={isExecutingAction}
                      onClick={() => void handleStartProxy()}
                    >
                      Start client proxy
                    </Button>
                  </div>
                ) : null}
                {check.check === "client-queue" && snapshot.repair_enabled ? (
                  <div className="mt-2">
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="text-xs h-7"
                      disabled={isExecutingAction}
                      onClick={() => void handleRepairQueue()}
                    >
                      Repair Client queue
                    </Button>
                  </div>
                ) : null}
                {check.check === "server-identity" &&
                (check.status === "failed" || check.code === "server-identity-changed" || check.code === "server-untrusted") &&
                onReviewServer ? (
                  <div className="mt-2">
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="text-xs h-7"
                      onClick={() => onReviewServer(target.server_address)}
                    >
                      Review server identity
                    </Button>
                  </div>
                ) : null}
                {check.check === "network-channel" &&
                (check.status === "failed" || check.status === "inconclusive" || check.code === "not-authorized") &&
                onConfigureNetworkChannel ? (
                  <div className="mt-2">
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="text-xs h-7"
                      onClick={onConfigureNetworkChannel}
                    >
                      Network Channel settings
                    </Button>
                  </div>
                ) : null}
              </li>
            ))}
          </ul>
          {!snapshot.queue_empty ? <p className="mt-3 text-sm">Testing and repair are blocked until this Client queue is known to be empty.</p> : null}
          <p className="mt-3 text-sm text-muted-foreground">To inspect pending work, open Windows Settings &gt; Printers &amp; scanners &gt; your printer &gt; Open print queue, or your Linux desktop&apos;s printer queue. Wait for jobs to finish or manage them there.</p>
          {snapshot.attempted_actions.length > 0 ? (
            <div className="mt-4 rounded-md border p-3 text-sm bg-muted/40" aria-label="Attempted recovery actions">
              <h4 className="font-semibold mb-2">Attempted recovery actions</h4>
              <ul className="space-y-1">
                {snapshot.attempted_actions.map((item, idx) => (
                  <li key={idx} className="flex justify-between items-center text-xs">
                    <span className="font-medium">{ACTION_LABELS[item.action] ?? item.action}</span>
                    <span className="text-muted-foreground">{OUTCOME_LABELS[item.outcome] ?? item.outcome}</span>
                  </li>
                ))}
              </ul>
              <p className="mt-2 text-xs text-muted-foreground">
                Attempted recovery actions do not claim physical printing is restored.
              </p>
            </div>
          ) : null}
          {isExecutingAction ? (
            <p className="mt-2 text-xs text-muted-foreground" role="status">
              Performing recovery action...
            </p>
          ) : null}
        </> : null}
      </div>
      <p className="mt-3 text-xs text-muted-foreground">These local checks do not verify the Server or physical output. Closing discards this session and does not cancel existing print jobs.</p>
      <div className="flex justify-end gap-2 mt-4">
        <Button
          type="button"
          variant="outline"
          disabled={isExecutingAction}
          onClick={() => setRun((value) => value + 1)}
        >
          Check again
        </Button>
        <Button type="button" onClick={onClose}>Close troubleshooting</Button>
      </div>
    </Dialog>
  );
}
