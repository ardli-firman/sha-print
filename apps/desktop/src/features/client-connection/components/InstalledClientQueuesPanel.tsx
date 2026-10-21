import { useEffect, useState } from "react";
import { Check, Copy, ExternalLink, Plus, RefreshCw, ShieldAlert, Wrench } from "lucide-react";
import { Button } from "../../../components/ui/button";
import {
  inspectServerConnection,
  openPrintersSettings,
  repairPrinterQueue,
} from "../../../api/serverConnections";
import type { RecognisedClientQueue } from "../../../api/types";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../../../components/ui/dialog";
import type { UseRecognisedClientQueues } from "../hooks/useRecognisedClientQueues";

interface InstalledClientQueuesPanelProps {
  clientQueues: UseRecognisedClientQueues;
  onAddPrinter: () => void;
  onReverifyServer?: (serverAddress: string) => void;
  onTroubleshoot?: (queue: RecognisedClientQueue) => void;
}

export function InstalledClientQueuesPanel({
  clientQueues,
  onAddPrinter,
  onReverifyServer,
  onTroubleshoot,
}: InstalledClientQueuesPanelProps) {
  const { queues, loading, error, refresh } = clientQueues;
  const [copiedQueue, setCopiedQueue] = useState<string | null>(null);
  const [manageError, setManageError] = useState<string | null>(null);
  const [serverChangedMap, setServerChangedMap] = useState<Record<string, boolean>>({});
  const [repairTarget, setRepairTarget] = useState<RecognisedClientQueue | null>(null);
  const [repairing, setRepairing] = useState(false);
  const [repairError, setRepairError] = useState<string | null>(null);
  const [repairSuccess, setRepairSuccess] = useState<string | null>(null);

  // Check known server identities for installed queues with evidence attribution
  useEffect(() => {
    let active = true;

    async function checkServerIdentities() {
      if (queues.length === 0) return;
      const uniqueAddresses = Array.from(new Set(queues.map((q) => q.server_address)));

      for (const address of uniqueAddresses) {
        try {
          const review = await inspectServerConnection(address);
          if (active && review.previous_fingerprint && review.trusted === false) {
            setServerChangedMap((prev) => ({ ...prev, [address]: true }));
          } else if (active && review.trusted) {
            setServerChangedMap((prev) => ({ ...prev, [address]: false }));
          }
        } catch {
          // Unknown reachability must not speculate as offline or changed
        }
      }
    }

    void checkServerIdentities();

    return () => {
      active = false;
    };
  }, [queues]);

  async function handleCopyQueueName(queueName: string) {
    try {
      await navigator.clipboard.writeText(queueName);
      setCopiedQueue(queueName);
      setTimeout(() => setCopiedQueue(null), 2000);
    } catch {
      // Clipboard write failed
    }
  }

  async function handleManageInWindows() {
    setManageError(null);
    try {
      await openPrintersSettings();
    } catch (err) {
      setManageError(
        "Could not open Windows Settings automatically. Open Windows Settings > Bluetooth & devices > Printers & scanners to manage your installed queues.",
      );
    }
  }

  async function handleRepair() {
    if (!repairTarget) return;
    setRepairing(true);
    setRepairError(null);
    setRepairSuccess(null);
    try {
      await repairPrinterQueue(repairTarget.server_address, repairTarget.printer_name);
      const repairedName = repairTarget.queue_name;
      setRepairTarget(null);
      setRepairSuccess(`Repaired ${repairedName}. Windows reloaded its paper sizes.`);
      await refresh();
    } catch (cause) {
      setRepairError(
        typeof cause === "object" && cause !== null && "message" in cause
          ? String((cause as { message: unknown }).message)
          : "Could not repair the Windows printer queue.",
      );
    } finally {
      setRepairing(false);
    }
  }

  return (
    <section className="card space-y-4" aria-labelledby="installed-queues-title">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h2 id="installed-queues-title" className="text-base font-semibold">
            My printers
          </h2>
          <p className="text-xs text-muted-foreground">
            Printers available to your Windows applications through ShaPrint.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="default"
            size="sm"
            onClick={onAddPrinter}
            aria-label="Add printer"
          >
            <Plus className="size-3.5 mr-1" />
            Add printer
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void refresh()}
            disabled={loading}
            aria-label="Refresh installed queues"
          >
            <RefreshCw className={`size-3.5 mr-1 ${loading ? "animate-spin" : ""}`} />
            Refresh
          </Button>
        </div>
      </div>

      {loading && queues.length === 0 ? (
        <div className="rounded-lg border border-dashed p-4 text-center text-sm text-muted-foreground">
          Checking Windows printer queues…
        </div>
      ) : null}

      {error ? (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive flex items-center justify-between">
          <span>{error}</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void refresh()}
          >
            Retry
          </Button>
        </div>
      ) : null}

      {manageError ? (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-800 dark:text-amber-200">
          <p className="font-medium">{manageError}</p>
          <p className="mt-1 text-[11px] text-muted-foreground">
            Copy the queue name below to search for it directly in Windows.
          </p>
        </div>
      ) : null}

      {repairSuccess ? (
        <div className="rounded-lg border border-green-600/30 bg-green-600/10 p-3 text-xs text-green-800 dark:text-green-200">
          {repairSuccess}
        </div>
      ) : null}

      {!loading && !error && queues.length === 0 ? (
        <div className="rounded-lg border border-dashed p-8 text-center space-y-3">
          <p className="text-sm text-muted-foreground">
            No ShaPrint client queues installed in Windows.
          </p>
          <Button
            type="button"
            onClick={onAddPrinter}
            aria-label="Add a printer now"
          >
            <Plus className="size-4 mr-1.5" />
            Add printer
          </Button>
        </div>
      ) : null}

      {queues.length > 0 ? (
        <div className="space-y-3">
          <div className="flex items-center justify-between text-xs text-muted-foreground">
            <span>Windows-managed queues ({queues.length})</span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-7 text-xs text-primary"
              onClick={() => void handleManageInWindows()}
            >
              <ExternalLink className="size-3 mr-1" />
              Manage in Windows
            </Button>
          </div>

          <ul className="space-y-2" aria-label="Installed printer queues">
            {queues.map((queue) => {
              const identityChanged = serverChangedMap[queue.server_address] === true;

              return (
                <li
                  key={queue.queue_name}
                  className={`rounded-lg border p-3 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 ${
                    identityChanged ? "border-amber-500/50 bg-amber-500/5" : "bg-card"
                  }`}
                >
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <strong className="block text-sm font-medium">{queue.queue_name}</strong>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-6 px-1.5 text-xs text-muted-foreground hover:text-foreground"
                        onClick={() => void handleCopyQueueName(queue.queue_name)}
                        aria-label={`Copy queue name ${queue.queue_name}`}
                      >
                        {copiedQueue === queue.queue_name ? (
                          <Check className="size-3 text-green-600" />
                        ) : (
                          <Copy className="size-3" />
                        )}
                      </Button>
                    </div>
                    <div className="text-xs text-muted-foreground">
                      Shared printer: <span className="font-semibold">{queue.printer_name}</span> on{" "}
                      <span>{queue.server_address}</span>
                    </div>

                    {identityChanged ? (
                      <div className="flex items-center gap-1.5 text-xs text-amber-700 dark:text-amber-300 font-medium pt-1">
                        <ShieldAlert className="size-3.5 shrink-0" />
                        <span>Server identity changed. Printing is blocked until reverified.</span>
                      </div>
                    ) : null}
                  </div>

                  <div className="flex items-center gap-2 self-end sm:self-auto">
                    {onTroubleshoot ? <Button type="button" variant="outline" size="sm"
                      onClick={() => onTroubleshoot(queue)} aria-label={`Troubleshoot ${queue.queue_name}`}>
                      Troubleshoot
                    </Button> : null}
                    {identityChanged && onReverifyServer ? (
                      <Button
                        type="button"
                        variant="default"
                        size="sm"
                        className="h-8 text-xs bg-amber-600 hover:bg-amber-700 text-white"
                        onClick={() => onReverifyServer(queue.server_address)}
                        aria-label={`Reverify identity for ${queue.server_address}`}
                      >
                        <ShieldAlert className="size-3 mr-1" />
                        Reverify identity
                      </Button>
                    ) : null}

                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8 text-xs"
                      disabled={identityChanged}
                      onClick={() => {
                        setRepairError(null);
                        setRepairTarget(queue);
                      }}
                      aria-label={`Repair paper sizes for ${queue.queue_name}`}
                    >
                      <Wrench className="size-3 mr-1" />
                      Repair paper sizes
                    </Button>

                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8 text-xs"
                      onClick={() => void handleManageInWindows()}
                    >
                      <ExternalLink className="size-3 mr-1" />
                      Manage in Windows
                    </Button>
                  </div>
                </li>
              );
            })}
          </ul>
        </div>
      ) : null}

      <Dialog
        open={repairTarget !== null}
        onOpenChange={(open) => {
          if (!open && !repairing) {
            setRepairTarget(null);
            setRepairError(null);
          }
        }}
        ariaLabelledBy="repair-queue-title"
        ariaDescribedBy="repair-queue-description"
      >
        <DialogHeader>
          <DialogTitle id="repair-queue-title">Repair paper sizes?</DialogTitle>
          <DialogDescription id="repair-queue-description">
            ShaPrint will remove and reinstall {repairTarget?.queue_name}. Windows will request
            administrator permission and may reset this printer&apos;s local printing preferences.
            Pending print jobs must finish or be cancelled first.
          </DialogDescription>
        </DialogHeader>
        {repairError ? (
          <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
            {repairError}
          </div>
        ) : null}
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={repairing}
            onClick={() => {
              setRepairTarget(null);
              setRepairError(null);
            }}
          >
            Cancel
          </Button>
          <Button type="button" disabled={repairing} onClick={() => void handleRepair()}>
            {repairing ? "Repairing…" : "Repair queue"}
          </Button>
        </DialogFooter>
      </Dialog>
    </section>
  );
}
