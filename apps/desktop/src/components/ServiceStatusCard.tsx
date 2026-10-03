import { printRecovery } from "@/api/availability";
import type { ServiceId, ServiceState, ServiceStatus } from "@/api/types";
import { Badge, type BadgeProps } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

/** User-facing name and purpose of each supervised service. */
const SERVICE_LABELS: Partial<Record<ServiceId, { title: string; description: string }>> = {
  "client-proxy": {
    title: "Client proxy",
    description: "Forwards print jobs from your installed queues to a trusted server.",
  },
  "server-sharing": {
    title: "Server sharing",
    description: "Shares the local printer queues you selected with other ShaPrint users.",
  },
  "server-discovery": {
    title: "Server discovery",
    description: "Finds ShaPrint servers on this network so you can review and connect to them.",
  },
};

const STATE_LABELS: Record<ServiceState, string> = {
  stopped: "Stopped",
  starting: "Starting",
  running: "Running",
  stopping: "Stopping",
  failed: "Failed",
};

const STATE_VARIANTS: Record<ServiceState, NonNullable<BadgeProps["variant"]>> = {
  stopped: "muted",
  starting: "warning",
  running: "success",
  stopping: "warning",
  failed: "destructive",
};

/** Which control is legal in each state; the shell rejects anything else as `invalid-state`. */
const CONTROLS: Record<ServiceState, { canStart: boolean; canStop: boolean }> = {
  stopped: { canStart: true, canStop: false },
  starting: { canStart: false, canStop: true },
  running: { canStart: false, canStop: true },
  stopping: { canStart: false, canStop: false },
  failed: { canStart: true, canStop: false },
};

interface ServiceStatusCardProps {
  service: ServiceStatus;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
}

/** One service with its live state and the controls that own its lifecycle. */
export function ServiceStatusCard({ service, busy, onStart, onStop }: ServiceStatusCardProps) {
  const label = SERVICE_LABELS[service.id];
  const controls = CONTROLS[service.state];
  const recovery = printRecovery(service);

  return (
    <section className="service" data-service={service.id} data-state={service.state}>
      <header className="service-header">
        <div>
          <h3>{label?.title ?? service.id}</h3>
          {label ? <p className="service-description">{label.description}</p> : null}
        </div>
        <Badge className="service-state-badge" variant={STATE_VARIANTS[service.state]} role="status">
          {STATE_LABELS[service.state] ?? service.state}
        </Badge>
      </header>

      {service.detail ? <p className="service-detail">{service.detail}</p> : null}

      {recovery ? (
        <p className="service-recovery" role="status">
          <span className="service-recovery-label">Printing affected</span>
          {recovery}
        </p>
      ) : null}

      <div className="service-actions">
        <Button type="button" size="sm" onClick={onStart} disabled={busy || !controls.canStart}>
          Start
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onStop}
          disabled={busy || !controls.canStop}
        >
          Stop
        </Button>
      </div>
    </section>
  );
}
