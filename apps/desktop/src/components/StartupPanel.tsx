import type { StartupController } from "../hooks/useStartup";

interface StartupPanelProps {
  startup: StartupController;
}

/**
 * Login startup and what closing the window means. Both belong together: the app starts with the
 * user's session so installed queues keep reaching the client proxy, and its window closes into the
 * tray rather than ending that.
 */
export function StartupPanel({ startup }: StartupPanelProps) {
  const { status, error, busy, setEnabled } = startup;

  return (
    <section className="panel" aria-labelledby="startup-heading">
      <div className="panel-header">
        <h2 id="startup-heading">Startup</h2>
      </div>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
        </p>
      ) : null}

      {status === null ? <p className="hint">Reading the login setting…</p> : null}

      {status && !status.supported ? (
        <p className="hint">
          Starting ShaPrint at login is available on Windows. Closing the window ends the app on this
          platform.
        </p>
      ) : null}

      {status?.supported ? (
        <>
          <label className="startup-choice">
            <input
              type="checkbox"
              checked={status.enabled}
              disabled={busy}
              onChange={(event) => void setEnabled(event.target.checked)}
            />
            <span>Start ShaPrint when I sign in to Windows</span>
          </label>
          {status.command ? (
            <p className="startup-command">
              Windows runs <code>{status.command}</code>
            </p>
          ) : (
            <p className="hint">
              ShaPrint cannot locate its program, so it cannot register itself to start at login.
            </p>
          )}
          <p className="hint">
            Closing the window keeps ShaPrint running in the notification area so your installed
            queues still print. Use <strong>Quit ShaPrint</strong> in that menu to stop it.
          </p>
        </>
      ) : null}
    </section>
  );
}
