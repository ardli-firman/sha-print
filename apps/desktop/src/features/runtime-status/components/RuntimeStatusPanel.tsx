import { RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import type { RuntimeStatusController } from "../hooks/useRuntimeStatus";
import { ServiceStatusCard } from "./ServiceStatusCard";

export interface RuntimeStatusPanelProps {
  runtime: RuntimeStatusController;
}

/** Live status of the client and server print paths, with lifecycle controls. */
export function RuntimeStatusPanel({ runtime }: RuntimeStatusPanelProps) {
  const { status, error, busy, unreachable, dismissError } = runtime;

  return (
    <Card className="panel feature-panel runtime-panel" aria-labelledby="services-heading">
      <div className="panel-header">
        <div>
          <h2 id="services-heading">Print services</h2>
          <p className="hint">
            The client proxy carries your installed queues. Server sharing stays off until you start it.
          </p>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => void runtime.refresh()}
          disabled={busy !== null}
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

      {unreachable ? (
        <p className="runtime-unreachable" role="alert">
          The ShaPrint runtime is not reachable from this window. Reopen the desktop app to see and
          control its services.
        </p>
      ) : null}

      {status === null && !unreachable ? (
        <p className="hint" role="status">Reading service status…</p>
      ) : null}

      {status ? (
        <ul className="service-list">
          {status.services.map((service) => (
            <li key={service.id}>
              <ServiceStatusCard
                service={service}
                busy={busy === service.id}
                onStart={() => void runtime.start(service.id)}
                onStop={() => void runtime.stop(service.id)}
              />
            </li>
          ))}
        </ul>
      ) : null}
    </Card>
  );
}
