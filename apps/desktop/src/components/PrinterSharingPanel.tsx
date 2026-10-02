import type { PrinterSharingController } from "../hooks/usePrinterSharing";

interface PrinterSharingPanelProps {
  sharing: PrinterSharingController;
}

/**
 * The server side of sharing: which local queues other ShaPrint users may print to, the
 * certificate identity they approve, and the one server-side action that needs administrator
 * permission. Installing a client queue is the other action that prompts, in the panel below.
 *
 * Sharing itself is started and stopped on the Server sharing service card, so a change here only
 * decides what a running server exposes.
 */
export function PrinterSharingPanel({ sharing }: PrinterSharingPanelProps) {
  const { printers, identity, busy, granting, error, toggle, allowAccess, refresh, dismissError } =
    sharing;
  const sharedCount = printers?.filter((printer) => printer.shared).length ?? 0;
  const changing = busy !== null || granting;

  return (
    <section className="panel" aria-labelledby="sharing-heading">
      <div className="panel-header">
        <h2 id="sharing-heading">Shared printers</h2>
        <button type="button" onClick={() => void refresh()} disabled={changing}>
          Refresh
        </button>
      </div>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
          <button type="button" className="banner-dismiss" onClick={dismissError}>
            Dismiss
          </button>
        </p>
      ) : null}

      {printers === null ? <p className="hint">Reading the local printer queues…</p> : null}

      {printers !== null && printers.length === 0 ? (
        <p className="hint">This computer has no local printer queue to share.</p>
      ) : null}

      {printers !== null && printers.length > 0 ? (
        <>
          <p className="hint">
            {sharedCount === 0
              ? "No queue is shared yet. Select the queues other ShaPrint users may print to, then start server sharing."
              : `${sharedCount} of ${printers.length} local queues are shared while server sharing runs.`}
          </p>
          <ul className="printer-list">
            {printers.map((printer) => (
              <li
                key={printer.name}
                className="printer"
                data-printer={printer.name}
                data-shared={printer.shared}
              >
                <label className="printer-choice">
                  <input
                    type="checkbox"
                    checked={printer.shared}
                    disabled={changing}
                    onChange={() => void toggle(printer.name)}
                  />
                  <span className="printer-name">{printer.name}</span>
                </label>
                <span className={`badge ${printer.shared ? "badge-shared" : ""}`}>
                  {printer.shared ? "Shared" : "Not shared"}
                </span>
              </li>
            ))}
          </ul>
        </>
      ) : null}

      {identity ? (
        <div className="identity">
          <h3>Certificate fingerprint</h3>
          <p className="hint">
            A client user approves this fingerprint before printing here. Read it aloud or compare it
            with what the client shows.
          </p>
          <code className="fingerprint" data-testid="server-fingerprint">
            {identity.fingerprint}
          </code>
          <p className="hint">Clients connect to port {identity.port}.</p>
          <div className="service-actions">
            <button type="button" onClick={() => void allowAccess()} disabled={changing}>
              Allow client connections
            </button>
          </div>
          <p className="hint">
            This step asks Windows for administrator permission once: it opens the port for other
            computers. Selecting printers and starting or stopping sharing do not. Installing a
            client queue, in the panel below, is the only other step that prompts.
          </p>
        </div>
      ) : null}
    </section>
  );
}
