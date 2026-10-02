import type { LegacyChannelOutcome } from "../api/legacyImport";
import type { LegacyImportController } from "../hooks/useLegacyImport";

/** Short label for what happened to the previous app's Network Channel. */
const CHANNEL_LABELS: Record<LegacyChannelOutcome, string> = {
  importable: "Ready to import",
  imported: "Imported",
  "kept-existing": "Left alone",
  absent: "Nothing to import",
  unreadable: "Could not be opened",
};

interface LegacyImportPanelProps {
  legacy: LegacyImportController;
}

/**
 * The move from the previous .NET ShaPrint app: what this app took, what it left behind, and the
 * queues the user has to select again. The previous app's queue setup is never activated here.
 */
export function LegacyImportPanel({ legacy }: LegacyImportPanelProps) {
  const { report, error, busy, checkAgain } = legacy;

  return (
    <section className="panel" aria-labelledby="legacy-import-heading">
      <div className="panel-header">
        <h2 id="legacy-import-heading">Previous ShaPrint app</h2>
        {report?.found ? (
          <button type="button" onClick={() => void checkAgain()} disabled={busy}>
            Import again
          </button>
        ) : null}
      </div>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
        </p>
      ) : null}

      {report === null ? (
        <p className="hint">Checking for settings from the previous ShaPrint app…</p>
      ) : null}

      {report && !report.found ? (
        <p className="hint">No settings from a previous ShaPrint app were found on this computer.</p>
      ) : null}

      {report?.found ? (
        <>
          <div className="channel-outcome" data-channel={report.channel}>
            <span className="channel-outcome-label">
              Network Channel: {CHANNEL_LABELS[report.channel] ?? report.channel}
            </span>
            <p className="channel-outcome-note">{report.channel_note}</p>
          </div>

          {report.queues_need_reselection ? (
            <p className="reselect" role="status" data-testid="reselect-queues">
              <strong>Select your printers again.</strong> Choose the local queues to share in the
              Shared printers panel, and install client queues again from this app. The previous
              app&rsquo;s queue setup is not activated.
            </p>
          ) : null}

          {report.settings.length > 0 ? (
            <details className="left-behind">
              <summary>What this app did not take ({report.settings.length})</summary>
              <ul className="left-behind-list">
                {report.settings.map((setting) => (
                  <li key={setting.key} data-setting={setting.key}>
                    <span className="left-behind-label">{setting.label}</span>
                    <span className="left-behind-reason">{setting.reason}</span>
                  </li>
                ))}
              </ul>
            </details>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
