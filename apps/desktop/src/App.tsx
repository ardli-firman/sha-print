import { NearbyServersPanel } from "./components/NearbyServersPanel";
import { NetworkChannelPanel } from "./components/NetworkChannelPanel";
import { PrinterSharingPanel } from "./components/PrinterSharingPanel";
import { RuntimeStatusPanel } from "./components/RuntimeStatusPanel";
import { ServerConnectionsPanel } from "./components/ServerConnectionsPanel";
import { useNearbyServers } from "./hooks/useNearbyServers";
import { useNetworkChannel } from "./hooks/useNetworkChannel";
import { usePrinterSharing } from "./hooks/usePrinterSharing";
import { useRuntimeStatus } from "./hooks/useRuntimeStatus";
import { useServerConnections } from "./hooks/useServerConnections";
import "./App.css";

function App() {
  const runtime = useRuntimeStatus();
  const sharing = usePrinterSharing();
  const networkChannel = useNetworkChannel();
  const connections = useServerConnections();
  const nearby = useNearbyServers();

  return (
    <main className="app">
      <header className="app-header">
        <h1>ShaPrint</h1>
        <p className="tagline">Share printers and print through your normal Windows print dialog.</p>
      </header>

      <RuntimeStatusPanel runtime={runtime} />

      <PrinterSharingPanel sharing={sharing} />
      <NetworkChannelPanel settings={networkChannel} />
      <NearbyServersPanel nearby={nearby} connections={connections} />
      <ServerConnectionsPanel connections={connections} />

      <footer className="app-footer">
        Documents are submitted by Windows, not by this window: ShaPrint never shows or logs print
        job content.
      </footer>
    </main>
  );
}

export default App;
