import { Plus, RefreshCw } from "lucide-react";
import { Button } from "../../../components/ui/button";
import type { UseRecognisedClientQueues } from "../hooks/useRecognisedClientQueues";

interface InstalledClientQueuesPanelProps {
  clientQueues: UseRecognisedClientQueues;
  onAddPrinter: () => void;
}

export function InstalledClientQueuesPanel({
  clientQueues,
  onAddPrinter,
}: InstalledClientQueuesPanelProps) {
  const { queues, loading, error, refresh } = clientQueues;

  return (
    <section className="card space-y-4" aria-labelledby="installed-queues-title">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h2 id="installed-queues-title" className="text-base font-semibold">
            My printers
          </h2>
          <p className="text-xs text-muted-foreground">
            Printers available to your Windows applications through ShaPrint.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="default"
            size="sm"
            onClick={onAddPrinter}
            aria-label="Add printer"
          >
            <Plus className="size-3.5 mr-1" />
            Add printer
          </Button>
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
        <div className="rounded-lg border border-dashed p-8 text-center space-y-3">
          <p className="text-sm text-muted-foreground">
            No ShaPrint client queues installed in Windows.
          </p>
          <Button
            type="button"
            onClick={onAddPrinter}
            aria-label="Add a printer now"
          >
            <Plus className="size-4 mr-1.5" />
            Add printer
          </Button>
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
