import { useState } from "react";
import { Copy, RefreshCw } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import type { PrinterSharingController } from "@/hooks/usePrinterSharing";

interface PrinterSharingPanelProps {
  sharing: PrinterSharingController;
}

/** The server side of sharing: local queues, certificate identity, and firewall access. */
export function PrinterSharingPanel({ sharing }: PrinterSharingPanelProps) {
  const { printers, identity, busy, granting, error, toggle, allowAccess, refresh, dismissError } =
    sharing;
  const [copyMessage, setCopyMessage] = useState("");
  const sharedCount = printers?.filter((printer) => printer.shared).length ?? 0;
  const changing = busy !== null || granting;

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
    <Card className="panel sharing-panel" aria-labelledby="sharing-heading">
      <div className="panel-header">
        <div>
          <h2 id="sharing-heading">Local printers</h2>
          <p className="hint">Choose the queues clients are allowed to print to.</p>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => void refresh()}
          disabled={changing}
        >
          <RefreshCw size={15} aria-hidden="true" />
          Refresh
        </Button>
      </div>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
          <Button type="button" variant="ghost" size="sm" onClick={dismissError}>
            Dismiss
          </Button>
        </p>
      ) : null}

      {printers === null ? <p className="hint">Reading the local printer queues…</p> : null}

      {printers !== null && printers.length === 0 ? (
        <div className="empty-state">
          <span className="empty-state-mark" aria-hidden="true">—</span>
          <div>
            <strong>No local printers found</strong>
            <p className="hint">Install a printer in Windows, then refresh this list.</p>
          </div>
        </div>
      ) : null}

      {printers !== null && printers.length > 0 ? (
        <>
          <div className="printer-list-heading">
            <span>Printer queue</span>
            <span>{sharedCount} of {printers.length} shared</span>
          </div>
          <ul className="printer-list">
            {printers.map((printer) => (
              <li
                key={printer.name}
                className="printer"
                data-printer={printer.name}
                data-shared={printer.shared}
              >
                <div className="printer-choice">
                  <Checkbox
                    aria-label={`Share ${printer.name}`}
                    checked={printer.shared}
                    disabled={changing}
                    onCheckedChange={() => void toggle(printer.name)}
                  />
                  <span className="printer-name">{printer.name}</span>
                </div>
                <Badge variant={printer.shared ? "success" : "muted"}>
                  {printer.shared ? "Shared" : "Not shared"}
                </Badge>
              </li>
            ))}
          </ul>
          <p className="sharing-count">
            {sharedCount === 0
              ? "Select at least one queue, then start server sharing."
              : "Selected queues are available while server sharing is running."}
          </p>
        </>
      ) : null}

      {identity ? (
        <div className="identity">
          <div className="identity-heading">
            <div>
              <h3>Server identity</h3>
              <p className="hint">Clients verify this fingerprint before they can see shared printers.</p>
            </div>
            <Badge variant="info">Port {identity.port}</Badge>
          </div>
          <div className="fingerprint-row">
            <code className="fingerprint" data-testid="server-fingerprint">
              {identity.fingerprint}
            </code>
            <Button type="button" variant="outline" size="sm" onClick={() => void copyFingerprint()}>
              <Copy size={14} aria-hidden="true" />
              Copy
            </Button>
          </div>
          {copyMessage ? <p className="copy-message" role="status">{copyMessage}</p> : null}
          <div className="firewall-action">
            <div>
              <strong>Let clients reach this computer</strong>
              <p className="hint">
                Windows asks for administrator permission once to open the sharing port.
              </p>
            </div>
            <Button type="button" variant="outline" onClick={() => void allowAccess()} disabled={changing}>
              Allow client connections
            </Button>
          </div>
        </div>
      ) : null}
    </Card>
  );
}
