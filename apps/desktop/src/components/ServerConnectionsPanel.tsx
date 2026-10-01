import type { ServerConnectionsController } from "../hooks/useServerConnections";

export function ServerConnectionsPanel({ connections }: { connections: ServerConnectionsController }) {
  const { address, setAddress, review, result, installed, busy, error, inspect, approve, query, install } = connections;
  return <section className="panel" aria-labelledby="client-connections-heading">
    <div className="panel-header"><h2 id="client-connections-heading">Connect to a server</h2></div>
    <p className="hint">Enter a server host or host:port (default port 8631). The first review reads its certificate only; shared printers are not requested before you approve it.</p>
    <form onSubmit={event => { event.preventDefault(); void inspect(); }}>
      <label htmlFor="server-address">Server address</label>
      <div className="panel-header">
        <input id="server-address" value={address} onChange={event => setAddress(event.currentTarget.value)} placeholder="printer.local or 192.168.1.20:8631" autoComplete="off" disabled={busy !== null} />
        <button type="submit" disabled={busy !== null || address.trim() === ""}>{busy === "inspect" ? "Reviewing…" : "Inspect certificate"}</button>
      </div>
    </form>
    {error ? <p className="banner" role="alert">{error.message}</p> : null}
    {review ? <div className="identity" aria-live="polite">
      <p><strong>Server:</strong> {review.address}</p>
      {review.previous_fingerprint ? <p><strong>Previously approved fingerprint:</strong><br /><code>{review.previous_fingerprint}</code></p> : null}
      <p><strong>Currently presented SHA-256 fingerprint:</strong><br /><code>{review.current_fingerprint}</code></p>
      {review.trusted ? <>
        <p className="hint">This fingerprint matches the saved approval.</p>
        <button type="button" onClick={() => void query()} disabled={busy !== null}>{busy === "printers" ? "Loading printers…" : "Show shared printers"}</button>
      </> : <>
        <p className="hint">{review.previous_fingerprint ? "The identity changed. Printers are blocked. Reapprove only after verifying this new fingerprint with the server owner through a trusted channel." : "Review this fingerprint with the server owner through a trusted channel before approving."}</p>
        <button type="button" onClick={() => void approve()} disabled={busy !== null}>{busy === "approve" ? "Saving approval…" : review.previous_fingerprint ? "Explicitly reapprove this fingerprint" : "Approve this fingerprint"}</button>
      </>}
    </div> : null}
    {result ? <div aria-live="polite">
      <h3>Shared printers at {result.address}</h3>
      {result.printers.length ? <ul className="printer-list">{result.printers.map(name => <li key={name} className="printer" data-shared-printer={name}>
        <span className="printer-name">{name}</span>
        <button type="button" onClick={() => void install(name)} disabled={busy !== null} aria-label={`Install a Windows queue for ${name}`}>{busy === "install" ? "Installing…" : "Install Windows queue"}</button>
      </li>)}</ul> : <p className="hint">This server is not currently sharing any printers.</p>}
      <p className="hint">Installing creates a normal Windows printer queue that sends its jobs through this app's local proxy. Windows asks for administrator permission once; the queue keeps working after ShaPrint restarts.</p>
    </div> : null}
    {installed ? <p className="hint" role="status">Installed <strong>{installed.queue_name}</strong>. It now appears in your Windows print dialogs; start the client proxy before printing to {installed.printer_name}.</p> : null}
  </section>;
}
