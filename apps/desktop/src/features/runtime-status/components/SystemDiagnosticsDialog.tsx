import { useState } from "react";
import { Power, RefreshCw, Server } from "lucide-react";

import { printRecovery } from "@/api/availability";
import { shutdownDaemon } from "@/api/ipc";
import {
  toAppError,
  type AppError,
  type ControlMode,
  type ServiceId,
  type ServiceState,
  type ServiceStatus,
} from "@/api/types";
import { Badge, type BadgeProps } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { RuntimeStatusController } from "../hooks/useRuntimeStatus";

const SERVICE_LABELS: Record<ServiceId, { title: string; description: string }> = {
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
  starting: "Starting…",
  running: "Running",
  stopping: "Stopping…",
  failed: "Failed",
};

const STATE_VARIANTS: Record<ServiceState, NonNullable<BadgeProps["variant"]>> = {
  stopped: "muted",
  starting: "warning",
  running: "success",
  stopping: "warning",
  failed: "destructive",
};

const CONTROLS: Record<ServiceState, { canStart: boolean; canStop: boolean }> = {
  stopped: { canStart: true, canStop: false },
  starting: { canStart: false, canStop: true },
  running: { canStart: false, canStop: true },
  stopping: { canStart: false, canStop: false },
  failed: { canStart: true, canStop: false },
};

export interface SystemDiagnosticsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  runtime: RuntimeStatusController;
  /**
   * Control plane the shell administers. In `machine` mode the services belong to the system
   * daemon, so the dialog also offers stopping the daemon itself and explains its recovery.
   */
  controlMode?: ControlMode;
}

export function SystemDiagnosticsDialog({
  open,
  onOpenChange,
  runtime,
  controlMode = "local",
}: SystemDiagnosticsDialogProps) {
  const { status, error, busy, unreachable, refresh, start, stop, dismissError } = runtime;
  const [daemonBusy, setDaemonBusy] = useState(false);
  const [daemonError, setDaemonError] = useState<AppError | null>(null);
  const isMachine = controlMode === "machine";

  const stopDaemon = async () => {
    setDaemonBusy(true);
    setDaemonError(null);
    try {
      await shutdownDaemon();
      // The status stream reports the stopped daemon on its next poll.
    } catch (cause) {
      // A declined polkit prompt lands here with `not-authorized`; state was not touched.
      setDaemonError(toAppError(cause));
    } finally {
      setDaemonBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      ariaLabelledBy="diagnostics-title"
      ariaDescribedBy="diagnostics-description"
    >
      <DialogHeader>
        <DialogTitle id="diagnostics-title">Print service status</DialogTitle>
        <DialogDescription id="diagnostics-description">
          Check the client proxy and printer sharing here. The client proxy keeps installed queues
          working; server sharing starts only when you turn it on.
        </DialogDescription>
      </DialogHeader>

      <div className="space-y-4 my-2">
        {error ? (
          <div className="diagnostic-error" role="alert">
            <div>
              <span className="font-mono font-bold text-destructive mr-2">{error.code}</span>
              <span>{error.message}</span>
            </div>
            <Button size="sm" variant="ghost" onClick={dismissError}>
              Dismiss
            </Button>
          </div>
        ) : null}

        {daemonError ? (
          <div className="diagnostic-error" role="alert">
            <div>
              <span className="font-mono font-bold text-destructive mr-2">{daemonError.code}</span>
              <span>{daemonError.message}</span>
            </div>
            <Button size="sm" variant="ghost" onClick={() => setDaemonError(null)}>
              Dismiss
            </Button>
          </div>
        ) : null}

        {unreachable ? (
          <div className="runtime-unreachable" role="alert">
            <strong>{isMachine ? "Machine daemon unavailable" : "Runtime unavailable"}</strong>
            <p className="text-xs text-muted-foreground mt-1">
              {isMachine
                ? "The machine daemon is not responding, so its print services are stopped. Start the system service (sudo systemctl start shaprintd), then choose Refresh All."
                : "The ShaPrint runtime is not responding. Reopen the desktop app to reconnect."}
            </p>
          </div>
        ) : null}

        {status === null && !unreachable ? (
          <p className="diagnostic-loading" role="status">
            <RefreshCw className="size-4 text-primary" aria-hidden="true" />
            Checking print services…
          </p>
        ) : null}

        {status ? (
          <div className="space-y-3">
            {status.services.map((service: ServiceStatus) => {
              const label = SERVICE_LABELS[service.id];
              const controls = CONTROLS[service.state];
              const recovery = printRecovery(service);
              const isBusy = busy === service.id;

              return (
                <section
                  key={service.id}
                  className="diagnostic-service service"
                  data-service={service.id}
                  data-state={service.state}
                  aria-labelledby={`diagnostic-service-${service.id}`}
                >
                  <header className="service-header">
                    <div>
                      <div className="flex items-center gap-2">
                        <Server size={15} className="text-muted-foreground" aria-hidden="true" />
                        <h4 id={`diagnostic-service-${service.id}`} className="font-semibold text-sm">
                          {label?.title ?? service.id}
                        </h4>
                      </div>
                      <p className="service-description">{label?.description}</p>
                    </div>
                    <Badge variant={STATE_VARIANTS[service.state]} role="status">
                      {STATE_LABELS[service.state]}
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
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 text-xs px-3"
                      disabled={isBusy || !controls.canStart}
                      onClick={() => void start(service.id)}
                    >
                      {service.state === "starting" || (isBusy && service.state === "stopped")
                        ? "Starting…"
                        : "Start"}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-7 text-xs px-3 text-destructive hover:bg-destructive/10"
                      disabled={isBusy || !controls.canStop}
                      onClick={() => void stop(service.id)}
                    >
                      {service.state === "stopping" || (isBusy && service.state === "running")
                        ? "Stopping…"
                        : "Stop"}
                    </Button>
                  </div>
                </section>
              );
            })}
          </div>
        ) : null}

        {isMachine ? (
          <section
            className="diagnostic-service service"
            data-service="machine-daemon"
            aria-labelledby="diagnostic-daemon"
          >
            <header className="service-header">
              <div>
                <div className="flex items-center gap-2">
                  <Power size={15} className="text-muted-foreground" aria-hidden="true" />
                  <h4 id="diagnostic-daemon" className="font-semibold text-sm">
                    Machine daemon
                  </h4>
                </div>
                <p className="service-description">
                  Runs the Client proxy and Server sharing for this whole machine (shaprintd).
                </p>
              </div>
              <Badge variant="muted" role="status">
                System service
              </Badge>
            </header>

            <p className="service-detail">
              Stopping the daemon stops printing for every user on this machine. Start it again
              with <strong>sudo systemctl start shaprintd</strong>.
            </p>

            <div className="service-actions">
              <Button
                size="sm"
                variant="outline"
                className="h-7 text-xs px-3"
                disabled={daemonBusy}
                onClick={() => void stopDaemon()}
              >
                {daemonBusy ? "Stopping…" : "Stop ShaPrint daemon"}
              </Button>
            </div>
          </section>
        ) : null}
      </div>

      <DialogFooter className="flex items-center justify-between sm:justify-between w-full">
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="gap-1.5 text-xs"
          onClick={() => void refresh()}
          disabled={busy !== null}
        >
          <RefreshCw size={13} className={busy ? "animate-spin" : ""} />
          <span>Refresh All</span>
        </Button>
        <Button type="button" size="sm" onClick={() => onOpenChange(false)}>
          Close
        </Button>
      </DialogFooter>
    </Dialog>
  );
}
