import { Power } from "lucide-react";

import type { ControlMode } from "@/api/types";
import type { StartupController } from "../hooks/useStartup";

export interface StartupPanelProps {
  startup: StartupController;
  /**
   * Control plane the shell administers. In `machine` mode login startup belongs to the system
   * daemon package, so the panel reports that instead of a per-user Windows registration.
   */
  controlMode?: ControlMode;
}

/**
 * Login startup and what closing the window means. Both belong together: the app starts with the
 * user's session so installed queues keep reaching the client proxy, and its window closes into the
 * tray rather than ending that.
 */
export function StartupPanel({ startup, controlMode = "local" }: StartupPanelProps) {
  const { status, error, busy, setEnabled } = startup;
  const isMachine = controlMode === "machine";

  return (
    <section className="panel feature-panel startup-panel" aria-labelledby="startup-heading">
      <div className="panel-header flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <Power size={16} />
          </div>
          <div>
            <h2 id="startup-heading" className="text-base font-semibold leading-none">
              Startup
            </h2>
            <p className="hint text-xs mt-1 text-muted-foreground">
              {isMachine
                ? "The ShaPrint system service starts with this machine."
                : "Start ShaPrint when you sign in to Windows."}
            </p>
          </div>
        </div>
      </div>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
        </p>
      ) : null}

      {status === null ? (
        <p className="hint text-xs text-muted-foreground" role="status">
          Checking the login setting…
        </p>
      ) : null}

      {status && !status.supported ? (
        <p className="hint text-xs text-muted-foreground">
          {isMachine
            ? "The ShaPrint system service owns startup on this machine, so this window registers no login entry. Closing the window only closes the GUI — the Client proxy and Server keep printing."
            : "Starting ShaPrint at login is available on Windows. Closing the window ends the app on this platform."}
        </p>
      ) : null}

      {status?.supported ? (
        <div className="space-y-3 pt-1">
          <label className="startup-choice flex items-center gap-3 cursor-pointer select-none rounded-lg border border-border bg-card p-3 hover:bg-muted/40 transition-colors">
            <input
              type="checkbox"
              checked={status.enabled}
              disabled={busy}
              onChange={(event) => void setEnabled(event.target.checked)}
              className="size-4 rounded border-input text-primary focus:ring-primary"
            />
            <div className="flex flex-col">
              <span className="text-sm font-medium text-foreground">
                Start ShaPrint when I sign in to Windows
              </span>
              <span className="text-xs text-muted-foreground mt-0.5">
                Starts ShaPrint in the notification area so installed printer queues can keep using the client proxy.
              </span>
            </div>
          </label>

          {status.command ? (
            <details className="startup-details">
              <summary>View Windows startup entry</summary>
              <code className="startup-command">{status.command}</code>
            </details>
          ) : (
            <p className="hint text-xs text-muted-foreground">
              ShaPrint cannot locate its program, so it cannot register itself to start at login.
            </p>
          )}

          <p className="hint text-xs text-muted-foreground">
            Closing the window keeps ShaPrint running in the notification area so your installed
            queues still print. Use <strong>Quit ShaPrint</strong> in that menu to stop it.
          </p>
        </div>
      ) : null}
    </section>
  );
}
