import { ServiceStatusCard } from "./ServiceStatusCard";
import type { RuntimeStatusController } from "../hooks/useRuntimeStatus";

interface RuntimeStatusPanelProps {
  runtime: RuntimeStatusController;
}

/** Live status of every service the shell supervises, with the controls that own their lifetime. */
export function RuntimeStatusPanel({ runtime }: RuntimeStatusPanelProps) {
  const { status, error, busy, unreachable, dismissError } = runtime;

  return (
    <section className="panel" aria-labelledby="services-heading">
      <div className="panel-header">
        <h2 id="services-heading">Services</h2>
        <button type="button" onClick={() => void runtime.refresh()} disabled={busy !== null}>
          Refresh
        </button>
      </div>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
          <button type="button" className="banner-dismiss" onClick={dismissError}>
            Dismiss
          </button>
        </p>
      ) : null}

      {unreachable ? (
        <p className="hint">
          The ShaPrint runtime is not reachable from this window. Start the desktop app to see and
          control its services.
        </p>
      ) : null}

      {status === null && !unreachable ? <p className="hint">Reading service status…</p> : null}

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
    </section>
  );
}
