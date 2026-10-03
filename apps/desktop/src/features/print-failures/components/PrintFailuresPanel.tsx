import { AlertTriangle, CheckCircle, Clock, X } from "lucide-react";

import type { PrintFailurePath } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import type { PrintFailuresController } from "../hooks/usePrintFailures";

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

export interface PrintFailuresPanelProps {
  failures: PrintFailuresController;
}

/** The shell's explanation of the last failed job and its single recovery action. */
export function PrintFailuresPanel({ failures }: PrintFailuresPanelProps) {
  const { failure, error, dismiss } = failures;
  const observed = failure ? observedAt(failure.observed_at_ms) : null;
  const hasProblem = Boolean(failure || error);

  return (
    <Card
      className={`panel feature-panel print-problems-panel ${hasProblem ? "has-problem" : "is-clear"}`}
      aria-labelledby="print-failures-heading"
    >
      <div className="panel-header flex items-center justify-between">
        <div className="flex items-center gap-2">
          {hasProblem ? (
            <div className="flex size-6 items-center justify-center rounded-full bg-destructive/10 text-destructive">
              <AlertTriangle size={14} aria-hidden="true" />
            </div>
          ) : (
            <div className="flex size-4 items-center justify-center text-muted-foreground/60">
              <CheckCircle size={13} aria-hidden="true" />
            </div>
          )}
          <h2
            id="print-failures-heading"
            className={hasProblem ? "text-sm font-semibold text-destructive" : "text-xs font-medium text-muted-foreground"}
          >
            Print problems
          </h2>
        </div>

        {failure ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => void dismiss()}
            className="h-7 text-xs px-2 text-muted-foreground hover:text-foreground"
          >
            <X size={13} className="mr-1" />
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
        <article
          className="failure rounded-lg border border-destructive/30 bg-card p-3.5 space-y-2 mt-2"
          data-path={failure.path}
          data-code={failure.code}
        >
          <header className="failure-header flex items-center justify-between gap-2">
            <span className="failure-path text-xs font-semibold text-foreground">
              {PATH_LABELS[failure.path] ?? failure.path}
            </span>
            <Badge variant="destructive" className="failure-code font-mono text-[10px]">
              {failure.code}
            </Badge>
          </header>

          <p className="failure-message text-sm text-foreground">{failure.message}</p>

          <div className="failure-recovery rounded-md bg-muted/60 p-2.5 text-xs text-foreground space-y-0.5 border border-border/60">
            <strong className="text-primary font-semibold block text-xs">
              Try this
            </strong>
            <p className="text-xs leading-relaxed">{failure.recovery}</p>
          </div>

          <p className="failure-when text-[11px] text-muted-foreground flex items-center gap-1 pt-0.5">
            <Clock size={11} />
            <span>Reported </span>
            <time dateTime={observed?.iso ?? ""}>{observed?.text ?? "time unknown"}</time>
          </p>
        </article>
      ) : error ? null : (
        <p className="hint text-xs text-muted-foreground" role="status">
          No recent print failures.
        </p>
      )}
    </Card>
  );
}
