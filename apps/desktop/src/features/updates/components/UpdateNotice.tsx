import type { UpdateStatus } from "@/api/updates";

interface UpdateNoticeProps {
  status: UpdateStatus | null;
  error: string | null;
  checking: boolean;
  onCheck: () => void;
  onRestart: () => void;
}

export function UpdateNotice({ status, error, checking, onCheck, onRestart }: UpdateNoticeProps) {
  const update = status?.update;
  const ready = update?.state === "ready_to_restart";
  const failure = update?.state === "failed" ? update.message : error;
  if (!ready && !failure && update?.state !== "checking" && !checking) return null;

  return (
    <section className="update-notice" aria-label="Software updates">
      <div className="update-copy" aria-live="polite">
        {ready ? (
          <>
            <strong>ShaPrint {update.version} is ready</strong>
            <p>{status?.waiting_for_jobs
              ? "Your restart is queued. ShaPrint will restart when printing and spooler cleanup finish."
              : "The update is downloaded and verified. Restart when it suits you."}</p>
          </>
        ) : failure ? (
          <>
            <strong>Update could not finish</strong>
            <p role="alert">{failure}</p>
          </>
        ) : (
          <>
            <strong>Checking for updates</strong>
            <p>ShaPrint will keep printing available while it checks.</p>
          </>
        )}
      </div>
      <div className="update-actions">
        {ready ? (
          <button type="button" className="update-primary" onClick={onRestart}>
            {status?.waiting_for_jobs ? "Restart queued" : "Restart Now"}
          </button>
        ) : (
          <button type="button" onClick={onCheck} disabled={checking || update?.state === "checking"}>
            {checking || update?.state === "checking" ? "Checking…" : "Check for Updates"}
          </button>
        )}
      </div>
    </section>
  );
}
