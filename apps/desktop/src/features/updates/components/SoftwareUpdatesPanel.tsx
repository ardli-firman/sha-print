import { RefreshCw, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import type { useUpdates } from "../hooks/useUpdates";

export interface SoftwareUpdatesPanelProps {
  updates: ReturnType<typeof useUpdates>;
}

export function SoftwareUpdatesPanel({ updates }: SoftwareUpdatesPanelProps) {
  const { status, error, checking, check, restart } = updates;
  const update = status?.update;
  const isReady = update?.state === "ready_to_restart";

  return (
    <Card className="panel space-y-3" aria-labelledby="software-updates-heading">
      <div className="panel-header flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <Sparkles size={16} />
          </div>
          <div>
            <h2 id="software-updates-heading" className="text-base font-semibold leading-none">
              Software updates
            </h2>
            <p className="hint text-xs mt-1 text-muted-foreground">
              Current version {status?.current_version ? `v${status.current_version}` : "installed"}. Updates are downloaded and verified automatically.
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {isReady ? (
            <Button
              type="button"
              variant="default"
              size="sm"
              onClick={() => void restart()}
              className="text-xs h-8"
            >
              {status?.waiting_for_jobs ? "Restart queued" : "Restart Now"}
            </Button>
          ) : (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void check()}
              disabled={checking || update?.state === "checking"}
              className="text-xs h-8"
              aria-label="Check for updates"
            >
              <RefreshCw
                size={14}
                className={checking || update?.state === "checking" ? "animate-spin mr-1.5" : "mr-1.5"}
                aria-hidden="true"
              />
              {checking || update?.state === "checking" ? "Checking…" : "Check for updates"}
            </Button>
          )}
        </div>
      </div>

      {error ? (
        <p className="text-xs text-destructive font-medium" role="alert">
          {error}
        </p>
      ) : null}

      {isReady ? (
        <div className="rounded-md bg-emerald-500/10 border border-emerald-500/30 p-2.5 text-xs text-emerald-800 dark:text-emerald-200">
          <strong>ShaPrint {update.version} is ready to install.</strong>
          <p className="mt-0.5 text-muted-foreground">
            {status?.waiting_for_jobs
              ? "Your restart is queued while active print jobs drain."
              : "Restart ShaPrint to apply the latest release."}
          </p>
        </div>
      ) : null}
    </Card>
  );
}
