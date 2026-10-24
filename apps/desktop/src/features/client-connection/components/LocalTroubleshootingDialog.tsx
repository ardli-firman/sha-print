import { useEffect, useRef, useState } from "react";
import { startService } from "@/api/ipc";
import { repairPrinterQueue } from "@/api/serverConnections";
import { closeTestPrintSession, confirmTestPrint, refreshTestPrintSession, sendTestPrint, inspectLocalClientQueue, type LocalTroubleshootingSnapshot, type UserSymptom } from "@/api/troubleshooting";
import type { RecognisedClientQueue } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Dialog, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { buildTroubleshootingReport, CHECK_LABELS, actionLabel, actionOutcomeLabel, checkLabel, checkStatusLabel } from "@/lib/troubleshooting-report";
import { CLIENT_VERSION } from "@/lib/version";

const SYMPTOM_GUIDANCE: Record<UserSymptom, string> = {
  clipping: "Compare Requested media with Loaded media in the printer tray and Server queue. Inspect printable margins and the outer border patterns (2 mm, 5 mm, 10 mm) on the diagnostic page. This symptom guides investigation and does not prove a hardware fault.",
  shifted: "Compare Requested media with Loaded media and check the paper alignment guides in the printer tray. Inspect the 50 mm center alignment cross on the diagnostic page. This symptom guides investigation and does not prove an alignment fault.",
  "incorrect-size": "Compare Requested media with Loaded media in the printer tray and Server queue settings. Inspect page scaling and paper dimensions. This symptom guides investigation and does not prove a scaling fault.",
  blank: "Check the printer's ink, toner, or ribbon supplies and printhead condition. Try printing a native test page directly on the Server. Local acceptance confirms only spooler submission, not marking agent transfer. This symptom guides investigation and does not prove a communication fault.",
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
  const sessionIdRef = useRef<number | undefined>(undefined);
  const actionBusyRef = useRef(false);
  const copyBusyRef = useRef(false);
  const [testError, setTestError] = useState(false);
  const [testActionPending, setTestActionPending] = useState(false);
  const [symptom, setSymptom] = useState<UserSymptom | "">("");
  const [reportOpen, setReportOpen] = useState(false);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  const patchSnapshot = (patch: Partial<LocalTroubleshootingSnapshot>) => {
    setSnapshot((current) => (current ? { ...current, ...patch } : current));
  };
  // A copied indicator must not outlive the findings it describes.
  useEffect(() => {
    setCopyState("idle");
  }, [snapshot]);
  useEffect(() => () => {
    if (sessionIdRef.current !== undefined) void closeTestPrintSession(sessionIdRef.current).catch(() => {});
  }, [target]);

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
    const sessionId = sessionIdRef.current;
    const opening = sessionId === undefined;
    const checking = sessionId === undefined ? inspectLocalClientQueue(target) : refreshTestPrintSession(sessionId);
    void checking.then(
      (result) => {
        if (!active || settled) {
          if (opening && result.session_id !== undefined) void closeTestPrintSession(result.session_id).catch(() => {});
          return;
        }
        sessionIdRef.current = result.session_id;
        settled = true;
        window.clearTimeout(timer);
        setSnapshot({
          ...result,
          attempted_actions: [...attemptedActionsRef.current, ...result.attempted_actions],
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

  const handleTestAction = async (
    result?: "correct" | "no-output" | "incorrect",
    chosenSymptom?: UserSymptom,
  ) => {
    const id = sessionIdRef.current;
    if (id === undefined || actionBusyRef.current) return;
    actionBusyRef.current = true;
    setTestActionPending(true);
    setIsExecutingAction(true);
    setTestError(false);
    if (!result) {
      patchSnapshot({
        test_print_enabled: false,
        submission: "checking",
        physical_result: "unconfirmed",
        user_symptom: undefined,
      });
    }
    try {
      const next = result
        ? await confirmTestPrint(id, result, chosenSymptom)
        : await sendTestPrint(id);
      if (!activeRef.current || sessionIdRef.current !== id) return;
      setSnapshot({
        ...next,
        attempted_actions: [...attemptedActionsRef.current, ...next.attempted_actions],
      });
      // The select mirrors the recorded classification exactly: a send clears it.
      setSymptom(next.user_symptom ?? "");
    } catch {
      if (!activeRef.current) return;
      // A transport failure does not establish whether the spooler accepted work.
      if (!result) {
        patchSnapshot({
          test_print_enabled: false,
          submission: "uncertain",
          physical_result: "unconfirmed",
        });
      }
      setTestError(true);
    } finally {
      actionBusyRef.current = false;
      if (activeRef.current) {
        setIsExecutingAction(false);
        setTestActionPending(false);
      }
    }
  };

  const handleSymptomChange = (newSymptom: UserSymptom | "") => {
    // A pending confirmation owns the select; a change made now would be silently
    // dropped by the busy guard and then overwritten by the pending response.
    if (actionBusyRef.current) return;
    setSymptom(newSymptom);
    void handleTestAction("incorrect", newSymptom || undefined);
  };

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

  // Copying never falls back to another channel automatically: a failure is shown
  // so the user can select the preview text by hand instead.
  const handleCopyReport = async () => {
    if (!snapshot || copyBusyRef.current) return;
    copyBusyRef.current = true;
    try {
      await navigator.clipboard.writeText(buildTroubleshootingReport(snapshot));
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    } finally {
      copyBusyRef.current = false;
    }
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
                  {checkLabel(check.check)}{" "}
                  <span className="font-normal">
                    ({checkStatusLabel(check.status)})
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
          {snapshot.session_id !== undefined ? (
            <section className="mt-4 space-y-3 rounded-md border p-3 text-sm" aria-label="Guided test print">
              <p>Requested media: {snapshot.requested_media ?? "Unavailable"}</p>
              <p>One page, one copy, grayscale, one-sided. Saved queue defaults stay unchanged.</p>
              {snapshot.attempted_actions.some((action) => action.action === "send-test-print" || action.action === "retry-test-print") || snapshot.submission === "uncertain" ? <p>Before another send, check again and confirm the Client queue is empty. Empty local queue state does not establish downstream cancellation. Earlier work may still print downstream even when this queue is empty. A retry sends another page.</p> : null}
              <Button type="button" disabled={!snapshot.test_print_enabled || isExecutingAction} onClick={() => void handleTestAction()}>
                {snapshot.attempted_actions.some((action) => action.action === "send-test-print" || action.action === "retry-test-print") ? "Retry test print" : "Send test print"}
              </Button>
              {testActionPending ? <p role="status">Checking prerequisites and submitting. Do not send again.</p> : null}
              {testError ? <p role="alert">The result could not be confirmed. Inspect the native queue before checking again. No automatic retry was sent.</p> : null}
              {snapshot.submission === "accepted" ? <p>The native spooler accepted the test print. Physical output is unconfirmed until you respond.</p> : null}
              {snapshot.submission === "uncertain" ? <p>Submission is uncertain. The print job may still print. Physical output is unconfirmed until you respond. Do not assume that a timeout cancelled it.</p> : null}
              {snapshot.submission === "rejected" ? <p>The test print was rejected before a document was submitted. Inspect the native queue and its printing preferences, then check again.</p> : null}
              {snapshot.submission === "blocked" ? <p>Prerequisites changed. No test print was sent. Review the fresh checks and Requested media, then check again.</p> : null}
              {snapshot.submission === "accepted" || snapshot.submission === "uncertain" ? <>
                <p>What happened to the diagnostic page? Wait for delayed output before answering.</p>
                <div className="flex flex-wrap gap-2">
                  <Button type="button" disabled={isExecutingAction} onClick={() => void handleTestAction("correct")}>Printed correctly</Button>
                  <Button type="button" variant="outline" disabled={isExecutingAction} onClick={() => void handleTestAction("no-output")}>No output</Button>
                  <Button type="button" variant="outline" disabled={isExecutingAction} onClick={() => void handleTestAction("incorrect")}>Incorrect output</Button>
                </div>
              </> : null}
              {snapshot.physical_result === "correct" ? <p>You confirmed correct physical output.</p> : null}
              {snapshot.physical_result === "no-output" ? <p>You reported no output. Local spooler acceptance confirms only that work entered the local queue; it does not confirm whether the print job reached the Server or the physical printer. Inspect the native Client queue for pending work, check ShaPrint on the Server, and inspect the physical printer&apos;s status and paper tray. Wait for delayed output before sending again, as an earlier print job may still produce a page.</p> : null}
              {snapshot.physical_result === "incorrect" ? <>
                <label className="block">Observed symptom
                  <select aria-label="Observed symptom" value={symptom} onChange={(event) => handleSymptomChange(event.target.value as UserSymptom | "")} disabled={isExecutingAction} className="block border rounded p-2">
                    <option value="">Choose a symptom</option>
                    <option value="clipping">Clipping</option><option value="shifted">Shifted position</option>
                    <option value="incorrect-size">Incorrect size</option><option value="blank">Blank output</option>
                  </select>
                </label>
                {symptom ? <p>{SYMPTOM_GUIDANCE[symptom]}</p> : null}
              </> : null}
            </section>
          ) : null}
          {!snapshot.queue_empty ? <p className="mt-3 text-sm">Testing and repair are blocked until this Client queue is known to be empty.</p> : null}
          <p className="mt-3 text-sm text-muted-foreground">To inspect pending work, open Windows Settings &gt; Printers &amp; scanners &gt; your printer &gt; Open print queue, or your Linux desktop&apos;s printer queue. Wait for jobs to finish or manage them there.</p>
          {snapshot.attempted_actions.length > 0 ? (
            <div className="mt-4 rounded-md border p-3 text-sm bg-muted/40" aria-label="Attempted recovery actions">
              <h4 className="font-semibold mb-2">Attempted recovery actions</h4>
              <ul className="space-y-1">
                {snapshot.attempted_actions.map((item, idx) => (
                  <li key={idx} className="flex justify-between items-center text-xs">
                    <span className="font-medium">{actionLabel(item.action)}</span>
                    <span className="text-muted-foreground">{actionOutcomeLabel(item.outcome)}</span>
                  </li>
                ))}
              </ul>
              <p className="mt-2 text-xs text-muted-foreground">
                Attempted recovery actions do not claim physical printing is restored.
              </p>
            </div>
          ) : null}
          <div className="mt-4 rounded-md border p-3 text-sm bg-muted/40" aria-label="Troubleshooting report">
            <div className="flex flex-wrap items-center gap-2">
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="text-xs h-7"
                aria-expanded={reportOpen}
                onClick={() => setReportOpen((open) => !open)}
              >
                {reportOpen ? "Hide report" : "Preview report"}
              </Button>
              {reportOpen ? (
                <Button
                  type="button"
                  size="sm"
                  className="text-xs h-7"
                  onClick={() => void handleCopyReport()}
                >
                  Copy report
                </Button>
              ) : null}
            </div>
            {reportOpen ? (
              <>
                <pre className="mt-2 whitespace-pre-wrap text-xs" tabIndex={0}>
                  {buildTroubleshootingReport(snapshot)}
                </pre>
                {copyState === "copied" ? (
                  <p role="status" className="mt-2 text-xs">Report copied to the clipboard.</p>
                ) : null}
                {copyState === "failed" ? (
                  <p role="alert" className="mt-2 text-xs">Copy failed. Select the report text above and copy it manually.</p>
                ) : null}
              </>
            ) : null}
          </div>
          {isExecutingAction && !testActionPending ? (
            <p className="mt-2 text-xs text-muted-foreground" role="status">
              Performing recovery action...
            </p>
          ) : null}
        </> : null}
      </div>
      <p className="mt-3 text-xs text-muted-foreground">Checks show only observed evidence and do not confirm physical output. Closing discards this session and does not cancel submitted print jobs. Manage cancellation in the native OS queue. Local cancellation may not stop work already forwarded to the Server or printer.</p>
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
