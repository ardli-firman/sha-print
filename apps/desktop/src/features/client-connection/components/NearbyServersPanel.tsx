import { Laptop, Plus, Radio, RefreshCw } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { NearbyServersController } from "../hooks/useNearbyServers";
import type { ServerConnectionsController } from "../hooks/useServerConnections";

interface NearbyServersPanelProps {
  nearby: NearbyServersController;
  connections: ServerConnectionsController;
  onOpenWizard?: () => void;
}

/**
 * Servers found on this network. Acting on one only fills in the address and inspects its
 * certificate: discovery never replaces the fingerprint approval.
 */
export function NearbyServersPanel({
  nearby,
  connections,
  onOpenWizard,
}: NearbyServersPanelProps) {
  const { servers, error, dismissError } = nearby;

  return (
    <section className="panel feature-panel nearby-servers-panel" aria-labelledby="nearby-servers-heading">
      <div className="panel-header flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <Radio size={16} />
          </div>
          <div>
            <h2 id="nearby-servers-heading" className="text-base font-semibold leading-none">
              Nearby servers
            </h2>
            <p className="hint text-xs mt-1 text-muted-foreground">
              Found on this local network.
            </p>
          </div>
        </div>

        {onOpenWizard && (
          <Button
            type="button"
            size="sm"
            onClick={onOpenWizard}
            className="gap-1.5 text-xs font-semibold shadow-xs"
          >
            <Plus size={15} />
            <span>Guided setup</span>
          </Button>
        )}
      </div>

      <p className="hint discovery-scope">
        Discovery only reaches this subnet. Enter a server address below to connect across networks.
      </p>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="banner-dismiss h-7 text-xs px-2"
            onClick={dismissError}
          >
            Dismiss
          </Button>
        </p>
      ) : null}

      {servers.length === 0 ? (
        <div className="connection-empty" role="status">
          <strong>Nothing found on this network.</strong>
          <p>Enter a server address below to continue. Manual entry works across subnets.</p>
        </div>
      ) : (
        <ul
          className="nearby-list divide-y divide-border/60"
          aria-label="Nearby ShaPrint servers"
          aria-live="polite"
        >
          {servers.map((server) => (
            <li key={server.address} className="py-3 first:pt-1 last:pb-1">
              <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <Laptop size={16} className="text-primary shrink-0" />
                    <strong className="font-semibold text-sm text-foreground truncate">
                      {server.name}
                    </strong>
                    <Badge variant="muted" className="text-[10px]">
                      {server.printers.length} shared {server.printers.length === 1 ? "printer" : "printers"}
                    </Badge>
                  </div>
                  <p className="hint mt-1 text-xs text-muted-foreground">
                    <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-foreground">
                      {server.address}
                    </code>
                  </p>
                  <p className="hint mt-0.5 text-xs text-muted-foreground">
                    {server.printers.length > 0
                      ? `Shares: ${server.printers.join(", ")}`
                      : "Shares no printer right now"}
                  </p>
                </div>

                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="shrink-0 text-xs h-8 sm:self-center self-start"
                  onClick={() => void connections.reviewAddress(server.address)}
                  disabled={connections.busy !== null}
                >
                  {connections.busy === "inspect" && connections.address === server.address ? (
                    <>
                      <RefreshCw size={13} className="animate-spin mr-1" />
                      <span>Reviewing…</span>
                    </>
                  ) : (
                    "Review identity"
                  )}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
