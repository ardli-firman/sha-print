import { useId, useMemo, useState } from "react";
import { Copy, Printer, RefreshCw, Search, ShieldCheck } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import type { PrinterSharingController } from "../hooks/usePrinterSharing";

export interface PrinterSharingPanelProps {
  sharing: PrinterSharingController;
}

/** The server side of sharing: local queues and certificate identity. */
export function PrinterSharingPanel({ sharing }: PrinterSharingPanelProps) {
  const { printers, identity, busy, error, toggle, refresh, dismissError } = sharing;
  const [copyMessage, setCopyMessage] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const searchId = useId();

  const sharedCount = printers?.filter((printer) => printer.shared).length ?? 0;
  const changing = busy !== null;

  const filteredPrinters = useMemo(() => {
    if (!printers) return [];
    if (!searchQuery.trim()) return printers;
    return printers.filter((p) => p.name.toLowerCase().includes(searchQuery.toLowerCase().trim()));
  }, [printers, searchQuery]);

  async function copyFingerprint() {
    if (!identity) return;

    try {
      if (!navigator.clipboard?.writeText) {
        setCopyMessage("Clipboard access is unavailable. Select the fingerprint to copy it.");
        return;
      }
      await navigator.clipboard.writeText(identity.fingerprint);
      setCopyMessage("Fingerprint copied.");
    } catch {
      setCopyMessage("Could not copy the fingerprint. Select it to copy it manually.");
    }
  }

  return (
    <Card
      className="panel sharing-panel"
      aria-labelledby="sharing-heading"
      data-has-identity={identity ? "true" : "false"}
    >
      <div className="panel-header flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <Printer size={16} />
          </div>
          <div>
            <h2 id="sharing-heading" className="text-base font-semibold leading-none">
              Local printers
            </h2>
            <p className="hint text-xs mt-1 text-muted-foreground">
              Choose the queues clients are allowed to print to.
            </p>
          </div>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => void refresh()}
          disabled={changing}
          className="text-xs h-8"
        >
          <RefreshCw size={14} className={changing ? "animate-spin mr-1.5" : "mr-1.5"} aria-hidden="true" />
          Refresh
        </Button>
      </div>

      {error ? (
        <p className="banner sharing-error" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
          <Button type="button" variant="ghost" size="sm" onClick={dismissError}>
            Dismiss
          </Button>
        </p>
      ) : null}

      <div className="sharing-queue-area">
        {printers === null ? (
          <p className="hint text-xs text-muted-foreground" role="status">
            Reading the local printer queues…
          </p>
        ) : null}

        {printers !== null && printers.length === 0 ? (
          <div className="empty-state rounded-lg border border-dashed border-border/80 p-6 text-center text-xs">
            <span className="empty-state-mark mx-auto mb-2 flex size-8 items-center justify-center rounded-lg bg-muted text-muted-foreground" aria-hidden="true">—</span>
            <div>
              <strong className="text-sm font-semibold text-foreground">No local printers found</strong>
              <p className="hint mt-1 text-muted-foreground">Install a printer in Windows, then refresh this list.</p>
            </div>
          </div>
        ) : null}

        {printers !== null && printers.length > 0 ? (
          <div className="sharing-printers space-y-3">
            {printers.length > 5 && (
              <div className="relative">
                <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
                <Input
                  id={searchId}
                  aria-label="Search printers"
                  placeholder="Search printers"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="pl-8 text-xs h-8"
                />
              </div>
            )}

            <div className="printer-list-heading flex items-center justify-between text-xs font-semibold text-muted-foreground border-b border-border/60 pb-1.5 px-1">
              <span>Printer queue</span>
              <span className="font-mono text-[11px] bg-muted px-2 py-0.5 rounded-full text-foreground">
                {sharedCount} of {printers.length} shared
              </span>
            </div>

            {filteredPrinters.length > 0 ? (
              <ul className="printer-list divide-y divide-border/60">
                {filteredPrinters.map((printer, index) => {
                  const printerId = `${searchId}-queue-${index}`;
                  return (
                    <li
                      key={printer.name}
                      className="printer flex items-center justify-between py-2 px-1 hover:bg-muted/30 rounded-md transition-colors"
                      data-printer={printer.name}
                      data-shared={printer.shared}
                    >
                      <div className="printer-choice flex items-center gap-3 select-none">
                        <Checkbox
                          id={printerId}
                          aria-label={`Share ${printer.name}`}
                          checked={printer.shared}
                          disabled={changing}
                          onCheckedChange={() => void toggle(printer.name)}
                        />
                        <span className="printer-name text-sm font-medium text-foreground">{printer.name}</span>
                      </div>
                      <Badge variant={printer.shared ? "success" : "muted"} className="text-xs">
                        {printer.shared ? "Shared" : "Not shared"}
                      </Badge>
                    </li>
                  );
                })}
              </ul>
            ) : (
              <p className="hint printer-filter-empty" role="status">
                No printers match “{searchQuery}”.
              </p>
            )}

            <p className="sharing-count text-xs text-muted-foreground mt-1 px-1">
              {sharedCount === 0
                ? "Select at least one queue, then start server sharing."
                : "Selected queues are available while server sharing is running."}
            </p>
          </div>
        ) : null}
      </div>

      {identity ? (
        <div className="identity rounded-lg border border-border bg-card p-4 space-y-3 mt-2">
          <div className="identity-heading flex items-start justify-between gap-3">
            <div>
              <div className="flex items-center gap-2">
                <ShieldCheck size={16} className="text-primary" />
                <h3 className="text-sm font-semibold text-foreground">Server identity</h3>
              </div>
              <p className="hint text-xs text-muted-foreground mt-1">
                Clients verify this fingerprint before they can see shared printers.
              </p>
            </div>
            <Badge variant="info" className="text-xs font-mono">
              Port {identity.port}
            </Badge>
          </div>

          <div className="fingerprint-row flex flex-col sm:flex-row sm:items-center gap-2">
            <code
              className="fingerprint flex-1 rounded bg-muted/70 p-2 font-mono text-xs text-foreground tracking-wider break-all"
              data-testid="server-fingerprint"
            >
              {identity.fingerprint}
            </code>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void copyFingerprint()}
              className="shrink-0 text-xs h-8 gap-1.5 self-start sm:self-center"
            >
              <Copy size={13} aria-hidden="true" />
              Copy
            </Button>
          </div>

          {copyMessage ? (
            <p className="copy-message text-xs text-emerald-600 dark:text-emerald-400 font-medium" role="status">
              {copyMessage}
            </p>
          ) : null}

        </div>
      ) : null}
    </Card>
  );
}
