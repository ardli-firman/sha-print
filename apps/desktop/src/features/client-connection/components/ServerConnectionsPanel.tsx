import { Check, Copy, Network, Printer, RefreshCw, ShieldAlert } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { ServerConnectionsController } from "../hooks/useServerConnections";

export function ServerConnectionsPanel({
  connections,
}: {
  connections: ServerConnectionsController;
}) {
  const {
    address,
    setAddress,
    review,
    result,
    installed,
    busy,
    error,
    inspect,
    approve,
    query,
    install,
  } = connections;

  const [copied, setCopied] = useState(false);

  async function handleCopyFingerprint(fp: string) {
    try {
      await navigator.clipboard.writeText(fp);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // clipboard access error
    }
  }

  return (
    <section className="panel" aria-labelledby="client-connections-heading">
      <div className="panel-header flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <Network size={16} />
          </div>
          <div>
            <h2 id="client-connections-heading" className="text-base font-semibold leading-none">
              Connect to a server
            </h2>
            <p className="hint text-xs mt-1 text-muted-foreground">
              Direct connection by IP address or hostname.
            </p>
          </div>
        </div>
      </div>

      <p className="hint text-xs text-muted-foreground leading-relaxed">
        Enter a server host or host:port (default port 8631). The first review reads its certificate
        only; shared printers are not requested before you approve it.
      </p>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          void inspect();
        }}
        className="space-y-2"
      >
        <label htmlFor="server-address" className="text-xs font-semibold text-foreground block">
          Server address
        </label>
        <div className="flex flex-col sm:flex-row gap-2">
          <Input
            id="server-address"
            value={address}
            onChange={(event) => setAddress(event.currentTarget.value)}
            placeholder="printer.local or 192.168.1.20:8631"
            autoComplete="off"
            disabled={busy !== null}
            className="flex-1"
          />
          <Button
            type="submit"
            disabled={busy !== null || address.trim() === ""}
            className="shrink-0 font-medium"
          >
            {busy === "inspect" ? (
              <>
                <RefreshCw size={14} className="animate-spin mr-1.5" />
                <span>Reviewing…</span>
              </>
            ) : (
              "Inspect certificate"
            )}
          </Button>
        </div>
      </form>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
        </p>
      ) : null}

      {review ? (
        <div className="identity rounded-lg border border-border bg-card p-4 space-y-3" aria-live="polite">
          <div className="flex items-center justify-between">
            <p className="text-sm">
              <strong className="text-foreground">Server:</strong>{" "}
              <span className="font-mono text-muted-foreground">{review.address}</span>
            </p>
            <Badge variant={review.trusted ? "success" : "warning"}>
              {review.trusted ? "Approved" : "Needs Review"}
            </Badge>
          </div>

          {review.previous_fingerprint ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/10 p-2.5 text-xs text-destructive space-y-1">
              <div className="flex items-center gap-1.5 font-semibold">
                <ShieldAlert size={14} />
                <span>Previously approved fingerprint:</span>
              </div>
              <code className="block font-mono text-[11px] break-all bg-background/50 p-1 rounded">
                {review.previous_fingerprint}
              </code>
            </div>
          ) : null}

          <div className="space-y-1">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <strong>Currently presented SHA-256 fingerprint:</strong>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-6 px-2 text-[11px]"
                onClick={() => void handleCopyFingerprint(review.current_fingerprint)}
              >
                {copied ? <Check size={12} className="text-emerald-500 mr-1" /> : <Copy size={12} className="mr-1" />}
                {copied ? "Copied" : "Copy"}
              </Button>
            </div>
            <code className="block rounded bg-muted/70 p-2 font-mono text-xs text-foreground tracking-wider break-all">
              {review.current_fingerprint}
            </code>
          </div>

          {review.trusted ? (
            <div className="pt-2 flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-t border-border/70">
              <p className="hint text-xs text-muted-foreground">
                This fingerprint matches the saved approval.
              </p>
              <Button
                type="button"
                size="sm"
                onClick={() => void query()}
                disabled={busy !== null}
                className="shrink-0 text-xs"
              >
                {busy === "printers" ? (
                  <>
                    <RefreshCw size={13} className="animate-spin mr-1" />
                    <span>Loading printers…</span>
                  </>
                ) : (
                  "Show shared printers"
                )}
              </Button>
            </div>
          ) : (
            <div className="pt-2 space-y-2 border-t border-border/70">
              <p className="hint text-xs text-muted-foreground">
                {review.previous_fingerprint
                  ? "The identity changed. Printers are blocked. Reapprove only after verifying this new fingerprint with the server owner through a trusted channel."
                  : "Review this fingerprint with the server owner through a trusted channel before approving."}
              </p>
              <Button
                type="button"
                size="sm"
                onClick={() => void approve()}
                disabled={busy !== null}
                className="text-xs"
              >
                {busy === "approve"
                  ? "Saving approval…"
                  : review.previous_fingerprint
                    ? "Explicitly reapprove this fingerprint"
                    : "Approve this fingerprint"}
              </Button>
            </div>
          )}
        </div>
      ) : null}

      {result ? (
        <div aria-live="polite" className="space-y-3 pt-2">
          <div className="flex items-center gap-2">
            <Printer size={16} className="text-primary" />
            <h3 className="text-sm font-semibold">Shared printers at {result.address}</h3>
          </div>

          {result.printers.length ? (
            <ul className="printer-list divide-y divide-border/60">
              {result.printers.map((name) => (
                <li
                  key={name}
                  className="printer flex items-center justify-between py-2.5"
                  data-shared-printer={name}
                >
                  <span className="printer-name text-sm font-medium">{name}</span>
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => void install(name)}
                    disabled={busy !== null}
                    aria-label={`Install a Windows queue for ${name}`}
                    className="text-xs h-8"
                  >
                    {busy === "install" ? "Installing…" : "Install Windows queue"}
                  </Button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="hint text-xs text-muted-foreground">
              This server is not currently sharing any printers.
            </p>
          )}

          <p className="hint text-xs text-muted-foreground">
            Installing creates a normal Windows printer queue that sends its jobs through this
            app&apos;s local proxy. Windows asks for administrator permission once; the queue keeps
            working after ShaPrint restarts.
          </p>
        </div>
      ) : null}

      {installed ? (
        <p className="hint text-xs text-emerald-700 dark:text-emerald-400 bg-emerald-500/10 p-3 rounded-lg border border-emerald-500/20" role="status">
          Installed <strong>{installed.queue_name}</strong>. It now appears in your Windows print
          dialogs; start the client proxy before printing to {installed.printer_name}.
        </p>
      ) : null}
    </section>
  );
}
