import { useEffect, useState } from "react";
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

export function LocalTroubleshootingDialog({ target, onClose, onReviewServer, onConfigureNetworkChannel }: {
  target: RecognisedClientQueue;
  onClose: () => void;
  onReviewServer?: (serverAddress: string) => void;
  onConfigureNetworkChannel?: () => void;
}) {
  const [run, setRun] = useState(0);
  const [snapshot, setSnapshot] = useState<LocalTroubleshootingSnapshot | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    let active = true;
    let settled = false;
    setSnapshot(null);
    setError(false);
    const timer = window.setTimeout(() => {
      if (!active || settled) return;
      settled = true;
      setSnapshot({
        target, version: CLIENT_VERSION, queue_empty: false,
        test_print_enabled: false, repair_enabled: false, attempted_actions: [],
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
        setSnapshot(result);
      },
      () => {
        if (!active || settled) return;
        settled = true;
        window.clearTimeout(timer);
        setError(true);
      },
    );
    return () => { active = false; window.clearTimeout(timer); };
  }, [target, run]);

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
        </> : null}
      </div>
      <p className="mt-3 text-xs text-muted-foreground">These local checks do not verify the Server or physical output. Closing discards this session and does not cancel existing print jobs.</p>
      <div className="flex justify-end gap-2 mt-4">
        <Button type="button" variant="outline" onClick={() => setRun((value) => value + 1)}>Check again</Button>
        <Button type="button" onClick={onClose}>Close troubleshooting</Button>
      </div>
    </Dialog>
  );
}
