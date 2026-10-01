import type { NearbyServersController } from "../hooks/useNearbyServers";
import type { ServerConnectionsController } from "../hooks/useServerConnections";

interface NearbyServersPanelProps {
  nearby: NearbyServersController;
  connections: ServerConnectionsController;
}

/**
 * Servers found on this network. Acting on one only fills in the address and inspects its
 * certificate: discovery never replaces the fingerprint approval.
 */
export function NearbyServersPanel({ nearby, connections }: NearbyServersPanelProps) {
  const { servers, error, dismissError } = nearby;

  return (
    <section className="panel" aria-labelledby="nearby-servers-heading">
      <div className="panel-header">
        <h2 id="nearby-servers-heading">Nearby servers</h2>
      </div>
      <p className="hint">
        ShaPrint servers that advertise themselves on this network. Reviewing one shows its
        certificate fingerprint; shared printers are not requested before you approve it.
      </p>

      {error ? (
        <p className="banner" role="alert">
          <span className="banner-code">{error.code}</span>
          <span className="banner-message">{error.message}</span>
          <button type="button" className="banner-dismiss" onClick={dismissError}>
            Dismiss
          </button>
        </p>
      ) : null}

      {servers.length === 0 ? (
        <p className="hint">
          No ShaPrint server has advertised itself on this network yet. You can still connect by
          entering a server address below.
        </p>
      ) : (
        <ul className="nearby-list">
          {servers.map((server) => (
            <li key={server.instance} data-nearby={server.address}>
              <div className="panel-header">
                <div>
                  <strong>{server.name}</strong>
                  <p className="hint">
                    <code>{server.address}</code>
                  </p>
                  <p className="hint">
                    {server.printers.length > 0
                      ? `Shares: ${server.printers.join(", ")}`
                      : "Shares no printer right now"}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => void connections.reviewAddress(server.address)}
                  disabled={connections.busy !== null}
                >
                  {connections.busy === "inspect" ? "Reviewing…" : "Review certificate"}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
