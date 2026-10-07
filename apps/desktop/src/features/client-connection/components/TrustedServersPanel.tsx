import { Laptop, Printer, RefreshCw, ShieldAlert, ShieldCheck, Trash2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { TrustedServersController } from "../hooks/useTrustedServers";

interface TrustedServersPanelProps {
  trusted: TrustedServersController;
  onReviewServer: (address: string) => void;
}

export function TrustedServersPanel({ trusted, onReviewServer }: TrustedServersPanelProps) {
  const { servers, loading, refreshing, forgetting, error, refresh, forget } = trusted;

  return (
    <section className="panel feature-panel trusted-servers-panel" aria-labelledby="trusted-servers-heading">
      <div className="panel-header flex items-center gap-2.5">
        <div className="flex size-7 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <ShieldCheck size={16} />
        </div>
        <div>
          <h2 id="trusted-servers-heading" className="text-base font-semibold leading-none">
            Trusted servers
          </h2>
          <p className="hint mt-1 text-xs text-muted-foreground">
            Saved certificate approvals stay here, including servers on other networks.
          </p>
        </div>
      </div>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
        </p>
      ) : null}

      {loading && servers.length === 0 ? (
        <p className="connection-empty" role="status">
          Checking saved Trusted servers…
        </p>
      ) : servers.length === 0 ? (
        <div className="connection-empty" role="status">
          <strong>No Trusted servers yet.</strong>
          <p>Add a printer and approve its certificate to keep that server here.</p>
        </div>
      ) : (
        <ul className="trusted-server-list divide-y divide-border/60" aria-label="Trusted server directory">
          {servers.map((server) => {
            const isOnline = server.status === "online";
            const identityChanged = server.status === "identity_changed";
            const isRefreshing = refreshing[server.address] || server.status === "checking";
            const badge = isOnline
              ? { label: "Online", variant: "success" as const }
              : identityChanged
                ? { label: "Identity changed", variant: "warning" as const }
                : server.status === "checking"
                  ? { label: "Checking", variant: "muted" as const }
                  : { label: "Offline", variant: "warning" as const };

            return (
              <li key={server.address} className="trusted-server-row py-4 first:pt-1 last:pb-1">
                <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                  <div className="min-w-0 flex-1 space-y-2">
                    <div className="flex flex-wrap items-center gap-2">
                      <Laptop size={16} className="shrink-0 text-primary" />
                      <code className="break-all rounded bg-muted px-1.5 py-0.5 font-mono text-xs font-semibold text-foreground">
                        {server.address}
                      </code>
                      <Badge variant={badge.variant} role="status">
                        {badge.label}
                      </Badge>
                    </div>

                    {isOnline ? (
                      <>
                        <p className="hint flex items-center gap-1.5 text-xs text-emerald-700 dark:text-emerald-300">
                          <ShieldCheck size={14} /> Certificate matches the saved approval.
                        </p>
                        <div className="hint flex items-start gap-1.5 text-xs text-muted-foreground">
                          <Printer size={14} className="mt-0.5 shrink-0" />
                          <p>
                            {server.printers.length > 0
                              ? `Shared printers: ${server.printers.join(", ")}`
                              : "No shared printers currently."}
                          </p>
                        </div>
                      </>
                    ) : identityChanged ? (
                      <p className="hint flex items-start gap-1.5 text-xs text-amber-800 dark:text-amber-200" role="alert">
                        <ShieldAlert size={14} className="mt-0.5 shrink-0" />
                        <span>
                          The certificate changed. Saved fingerprint: {" "}
                          <code className="break-all font-mono">{server.fingerprint}</code>. Current fingerprint: {" "}
                          <code className="break-all font-mono">{server.current_fingerprint ?? "unavailable"}</code>.
                          Printer list is withheld until you review the identity.
                        </span>
                      </p>
                    ) : server.status === "offline" ? (
                      <p className="hint text-xs text-muted-foreground">
                        Server is unreachable. Its saved approval is kept.
                      </p>
                    ) : (
                      <p className="hint text-xs text-muted-foreground">Checking the saved fingerprint and shared printers…</p>
                    )}
                  </div>

                  <div className="flex flex-wrap items-center gap-2 lg:justify-end">
                    {isOnline || identityChanged ? (
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="text-xs"
                        onClick={() => onReviewServer(server.address)}
                        aria-label={`${identityChanged ? "Review identity" : "Review shared printers"} for ${server.address}`}
                      >
                        {identityChanged ? "Review identity" : "Review printers"}
                      </Button>
                    ) : null}
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="text-xs"
                      onClick={() => void refresh(server.address)}
                      disabled={isRefreshing || Boolean(forgetting[server.address])}
                      aria-label={`Refresh status for ${server.address}`}
                    >
                      <RefreshCw size={13} className={isRefreshing ? "animate-spin" : ""} />
                      Refresh
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="text-xs text-muted-foreground hover:text-destructive"
                      onClick={() => void forget(server.address)}
                      disabled={Boolean(forgetting[server.address]) || Boolean(refreshing[server.address])}
                      aria-label={`Forget ${server.address}`}
                    >
                      <Trash2 size={13} />
                      Forget
                    </Button>
                  </div>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
