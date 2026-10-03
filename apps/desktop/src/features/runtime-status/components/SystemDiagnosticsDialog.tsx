import { RefreshCw, Server, Wrench } from "lucide-react";

import { printRecovery } from "@/api/availability";
import type { ServiceId, ServiceState, ServiceStatus } from "@/api/types";
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
    title: "Client Proxy",
    description: "Accepts print jobs from Windows queues and tunnels them securely to the server.",
  },
  "server-sharing": {
    title: "Server Sharing (IPPS)",
    description: "Authenticates incoming print jobs via Network Channel and submits to local queues.",
  },
  "server-discovery": {
    title: "Server Discovery (mDNS)",
    description: "Broadcasts and detects ShaPrint servers on the local subnet.",
  },
};

const STATE_LABELS: Record<ServiceState, string> = {
  stopped: "Stopped",
  starting: "Starting...",
  running: "Running",
  stopping: "Stopping...",
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
}

export function SystemDiagnosticsDialog({
  open,
  onOpenChange,
  runtime,
}: SystemDiagnosticsDialogProps) {
  const { status, error, busy, unreachable, refresh, start, stop, dismissError } = runtime;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogHeader>
        <div className="flex items-center gap-2 text-primary font-semibold text-sm">
          <Wrench size={18} />
          <span>System Diagnostics</span>
        </div>
        <DialogTitle>Background Print Services</DialogTitle>
        <DialogDescription>
          ShaPrint automatically manages these services. You can inspect runtime health and
          manually restart daemons here if network issues occur.
        </DialogDescription>
      </DialogHeader>

      <div className="space-y-4 my-2">
        {error ? (
          <div className="rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-xs flex items-center justify-between">
            <div>
              <span className="font-mono font-bold text-destructive mr-2">{error.code}</span>
              <span>{error.message}</span>
            </div>
            <Button size="sm" variant="ghost" onClick={dismissError}>
              Dismiss
            </Button>
          </div>
        ) : null}

        {unreachable ? (
          <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive">
            <strong>Runtime Unreachable</strong>
            <p className="text-xs text-muted-foreground mt-1">
              The ShaPrint desktop core is not responding. Please relaunch the application.
            </p>
          </div>
        ) : null}

        {status === null && !unreachable ? (
          <div className="py-8 text-center text-sm text-muted-foreground flex flex-col items-center gap-2">
            <RefreshCw className="animate-spin size-5 text-primary" />
            <span>Inspecting service statuses...</span>
          </div>
        ) : null}

        {status ? (
          <div className="space-y-3">
            {status.services.map((service: ServiceStatus) => {
              const label = SERVICE_LABELS[service.id];
              const controls = CONTROLS[service.state];
              const recovery = printRecovery(service);
              const isBusy = busy === service.id;

              return (
                <div
                  key={service.id}
                  className="rounded-lg border border-border bg-card p-3.5 shadow-2xs space-y-2.5"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <div className="flex items-center gap-2">
                        <Server size={15} className="text-muted-foreground" />
                        <h4 className="font-semibold text-sm">{label?.title ?? service.id}</h4>
                      </div>
                      <p className="text-xs text-muted-foreground mt-0.5">{label?.description}</p>
                    </div>
                    <Badge variant={STATE_VARIANTS[service.state]}>
                      {STATE_LABELS[service.state]}
                    </Badge>
                  </div>

                  {service.detail ? (
                    <div className="rounded bg-muted/60 px-2.5 py-1 text-[11px] font-mono text-muted-foreground break-all">
                      {service.detail}
                    </div>
                  ) : null}

                  {recovery ? (
                    <div className="rounded-md border border-warning/30 bg-warning/10 p-2 text-xs text-warning-foreground">
                      <span className="font-bold block text-[11px]">Notice:</span>
                      {recovery}
                    </div>
                  ) : null}

                  <div className="flex items-center justify-end gap-2 pt-1">
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 text-xs px-3"
                      disabled={isBusy || !controls.canStart}
                      onClick={() => void start(service.id)}
                    >
                      {isBusy && service.state === "stopped" ? "Starting..." : "Start"}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-7 text-xs px-3 text-destructive hover:bg-destructive/10"
                      disabled={isBusy || !controls.canStop}
                      onClick={() => void stop(service.id)}
                    >
                      {isBusy && service.state === "running" ? "Stopping..." : "Stop"}
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
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
