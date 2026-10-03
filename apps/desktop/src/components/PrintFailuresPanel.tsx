import type { PrintFailurePath } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import type { PrintFailuresController } from "@/hooks/usePrintFailures";

/** Where a job stopped, in the user's terms. */
const PATH_LABELS: Record<PrintFailurePath, string> = {
  "client-forwarding": "Sending the job to the server",
  "server-submission": "Submitting the job to the printer queue",
};

function observedAt(milliseconds: number): { iso: string; text: string } {
  const date = new Date(milliseconds);
  if (Number.isNaN(date.getTime())) {
    return { iso: "", text: "time unknown" };
  }
  return { iso: date.toISOString(), text: date.toLocaleTimeString() };
}

interface PrintFailuresPanelProps {
  failures: PrintFailuresController;
}

/** The shell's explanation of the last failed job and its single recovery action. */
export function PrintFailuresPanel({ failures }: PrintFailuresPanelProps) {
  const { failure, error, dismiss } = failures;
  const observed = failure ? observedAt(failure.observed_at_ms) : null;
  const hasProblem = Boolean(failure || error);

  return (
    <Card
      className={`panel print-problems-panel ${hasProblem ? "has-problem" : "is-clear"}`}
      aria-labelledby="print-failures-heading"
    >
      <div className="panel-header">
        <h2 id="print-failures-heading">Print problems</h2>
        {failure ? (
          <Button type="button" variant="ghost" size="sm" onClick={() => void dismiss()}>
            Dismiss
          </Button>
        ) : null}
      </div>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
        </p>
      ) : null}

      {failure ? (
        <article className="failure" data-path={failure.path} data-code={failure.code}>
          <header className="failure-header">
            <span className="failure-path">{PATH_LABELS[failure.path] ?? failure.path}</span>
            <span className="failure-code">{failure.code}</span>
          </header>
          <p className="failure-message">{failure.message}</p>
          <p className="failure-recovery">
            <strong>Next step:</strong> {failure.recovery}
          </p>
          <p className="failure-when">
            Reported <time dateTime={observed?.iso ?? ""}>{observed?.text ?? "time unknown"}</time>
          </p>
        </article>
      ) : error ? null : (
        <p className="hint">
          No print problems reported. Jobs from installed queues print through the client proxy.
        </p>
      )}
    </Card>
  );
}
