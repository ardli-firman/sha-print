import { RefreshCw } from "lucide-react";
import { Button } from "../../../components/ui/button";
import type { UseRecognisedClientQueues } from "../hooks/useRecognisedClientQueues";

interface InstalledClientQueuesPanelProps {
  clientQueues: UseRecognisedClientQueues;
}

export function InstalledClientQueuesPanel({
  clientQueues,
}: InstalledClientQueuesPanelProps) {
  const { queues, loading, error, refresh } = clientQueues;

  return (
    <section className="card space-y-4" aria-labelledby="installed-queues-title">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h2 id="installed-queues-title" className="text-base font-semibold">
            Installed client queues
          </h2>
          <p className="text-xs text-muted-foreground">
            ShaPrint client queues currently installed in Windows.
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void refresh()}
          disabled={loading}
          aria-label="Refresh installed queues"
        >
          <RefreshCw className={`size-3.5 mr-1 ${loading ? "animate-spin" : ""}`} />
          Refresh
        </Button>
      </div>

      {loading && queues.length === 0 ? (
        <div className="rounded-lg border border-dashed p-4 text-center text-sm text-muted-foreground">
          Checking Windows printer queues…
        </div>
      ) : null}

      {error ? (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive flex items-center justify-between">
          <span>{error}</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void refresh()}
          >
            Retry
          </Button>
        </div>
      ) : null}

      {!loading && !error && queues.length === 0 ? (
        <div className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
          No ShaPrint client queues installed in Windows. Use Add printer to install one.
        </div>
      ) : null}

      {queues.length > 0 ? (
        <ul className="space-y-2" aria-label="Installed printer queues">
          {queues.map((queue) => (
            <li
              key={queue.queue_name}
              className="rounded-lg border bg-card p-3 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2"
            >
              <div>
                <strong className="block text-sm font-medium">{queue.queue_name}</strong>
                <span className="text-xs text-muted-foreground">
                  Shared printer: <span className="font-semibold">{queue.printer_name}</span> on{" "}
                  <span>{queue.server_address}</span>
                </span>
              </div>
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}
