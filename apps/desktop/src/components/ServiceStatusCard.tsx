import type { ServiceId, ServiceState, ServiceStatus } from "../api/types";

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

  return (
    <section className="service" data-service={service.id} data-state={service.state}>
      <header className="service-header">
        <div>
          <h3>{label?.title ?? service.id}</h3>
          {label ? <p className="service-description">{label.description}</p> : null}
        </div>
        <span className={`badge badge-${service.state}`} role="status">
          {STATE_LABELS[service.state] ?? service.state}
        </span>
      </header>

      {service.detail ? <p className="service-detail">{service.detail}</p> : null}

      <div className="service-actions">
        <button type="button" onClick={onStart} disabled={busy || !controls.canStart}>
          Start
        </button>
        <button type="button" onClick={onStop} disabled={busy || !controls.canStop}>
          Stop
        </button>
      </div>
    </section>
  );
}
