import { Network, Printer, Settings2, ShieldAlert, Wrench } from "lucide-react";
import { useState } from "react";

import { AddPrinterDialog } from "@/components/AddPrinterDialog";
import { LegacyImportPanel } from "@/components/LegacyImportPanel";
import { NearbyServersPanel } from "@/components/NearbyServersPanel";
import { NetworkChannelPanel } from "@/components/NetworkChannelPanel";
import { PrintFailuresPanel } from "@/components/PrintFailuresPanel";
import { PrinterSharingPanel } from "@/components/PrinterSharingPanel";
import { RuntimeStatusPanel } from "@/components/RuntimeStatusPanel";
import { ServerConnectionsPanel } from "@/components/ServerConnectionsPanel";
import { StartupPanel } from "@/components/StartupPanel";
import { SystemDiagnosticsDialog } from "@/components/SystemDiagnosticsDialog";
import { Button } from "@/components/ui/button";
import { useLegacyImport } from "@/hooks/useLegacyImport";
import { useNearbyServers } from "@/hooks/useNearbyServers";
import { useNetworkChannel } from "@/hooks/useNetworkChannel";
import { usePrintFailures } from "@/hooks/usePrintFailures";
import { usePrinterSharing } from "@/hooks/usePrinterSharing";
import { useRuntimeStatus } from "@/hooks/useRuntimeStatus";
import { useServerConnections } from "@/hooks/useServerConnections";
import { useStartup } from "@/hooks/useStartup";
import "./App.css";

const NAV_ITEMS = [
  { id: "share", label: "Share", icon: Printer },
  { id: "connect", label: "Connect", icon: Network },
  { id: "settings", label: "Settings", icon: Settings2 },
] as const;

type PageId = (typeof NAV_ITEMS)[number]["id"];

const PAGE_CONTENT: Record<PageId, { title: string; description: string }> = {
  share: {
    title: "Share printers",
    description: "Choose which local printer queues other ShaPrint users can print to.",
  },
  connect: {
    title: "Connect to a server",
    description: "Find a nearby server or enter its address, then review its identity before connecting.",
  },
  settings: {
    title: "Settings",
    description: "Manage print authorization, login startup, and settings from the previous app.",
  },
};

function App() {
  const [page, setPage] = useState<PageId>("share");
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [addPrinterOpen, setAddPrinterOpen] = useState(false);

  const runtime = useRuntimeStatus();
  const failures = usePrintFailures();
  const startup = useStartup();
  const legacy = useLegacyImport();
  const sharing = usePrinterSharing();
  const networkChannel = useNetworkChannel();
  const connections = useServerConnections();
  const nearby = useNearbyServers();
  const pageContent = PAGE_CONTENT[page];

  const hasFailedService =
    runtime.status?.services.some((s) => s.state === "failed") || runtime.error !== null;
  const isTransitioning =
    runtime.status?.services.some((s) => s.state === "starting" || s.state === "stopping") ||
    runtime.busy !== null;
  const isUnreachable = runtime.unreachable;

  return (
    <div className="app-shell">
      <aside className="sidebar" aria-label="ShaPrint navigation">
        <div className="brand-lockup">
          <span className="brand-mark" aria-hidden="true">
            <Printer size={19} strokeWidth={2.2} />
          </span>
          <span className="brand-copy">
            <span className="brand-name">ShaPrint</span>
            <span className="brand-caption">Windows print sharing</span>
          </span>
        </div>

        <nav className="primary-nav" aria-label="Primary navigation">
          <p className="nav-heading">Workspace</p>
          {NAV_ITEMS.map(({ id, label, icon: Icon }) => (
            <Button
              key={id}
              type="button"
              variant="ghost"
              className={`nav-button ${page === id ? "nav-button-active" : ""}`}
              aria-current={page === id ? "page" : undefined}
              title={label}
              onClick={() => setPage(id)}
            >
              <Icon aria-hidden="true" size={18} strokeWidth={1.9} />
              <span className="nav-label">{label}</span>
            </Button>
          ))}
        </nav>

        <div className="sidebar-note">
          <span className="sidebar-note-rule" aria-hidden="true" />
          <p>Print from the Windows dialog. ShaPrint never opens your documents.</p>
        </div>
      </aside>

      <main className="workspace">
        <header className="workspace-header">
          <div className="page-heading">
            <h1>{pageContent.title}</h1>
            <p className="page-description">{pageContent.description}</p>
          </div>

          <div className="workspace-header-actions">
            <button
              type="button"
              onClick={() => setDiagnosticsOpen(true)}
              className="system-status-pill"
              title="Open System Diagnostics"
            >
              <span
                className={`status-dot ${
                  isUnreachable || hasFailedService
                    ? "dot-failed"
                    : isTransitioning
                      ? "dot-busy"
                      : "dot-ready"
                }`}
              />
              <span className="status-label">
                {isUnreachable
                  ? "Runtime Offline"
                  : hasFailedService
                    ? "Service Degraded"
                    : isTransitioning
                      ? "Updating…"
                      : "Services Active"}
              </span>
              <Wrench size={13} className="status-icon" aria-hidden="true" />
            </button>
          </div>
        </header>

        <div className="workspace-content">
          <PrintFailuresPanel failures={failures} />

          <section className="page-stack" aria-label={pageContent.title}>
            {page === "share" ? (
              <>
                {networkChannel.configured === false ? (
                  <div className="channel-reminder" role="status">
                    <ShieldAlert size={18} aria-hidden="true" />
                    <div>
                      <strong>Set a Network Channel before clients can print.</strong>
                      <p>This shared value authorizes print jobs between your ShaPrint installations.</p>
                    </div>
                    <Button type="button" variant="outline" onClick={() => setPage("settings")}>
                      Set up
                    </Button>
                  </div>
                ) : null}
                <PrinterSharingPanel sharing={sharing} />
              </>
            ) : null}

            {page === "connect" ? (
              <>
                <NearbyServersPanel
                  nearby={nearby}
                  connections={connections}
                  onOpenWizard={() => setAddPrinterOpen(true)}
                />
                <ServerConnectionsPanel connections={connections} />
              </>
            ) : null}

            {page === "settings" ? (
              <>
                <NetworkChannelPanel settings={networkChannel} />
                <StartupPanel startup={startup} />
                <RuntimeStatusPanel runtime={runtime} />
                <LegacyImportPanel legacy={legacy} />
              </>
            ) : null}
          </section>
        </div>

        <footer className="workspace-footer">
          Documents stay on your devices. ShaPrint never displays or logs print-job content.
        </footer>
      </main>

      {/* Wizard and Diagnostics Dialogs */}
      <AddPrinterDialog
        open={addPrinterOpen}
        onOpenChange={setAddPrinterOpen}
        nearby={nearby}
        connections={connections}
        networkChannel={networkChannel}
        onEnsureProxyRunning={() => {
          const proxy = runtime.status?.services.find((s) => s.id === "client-proxy");
          if (proxy && proxy.state !== "running") {
            void runtime.start("client-proxy");
          }
        }}
      />

      <SystemDiagnosticsDialog
        open={diagnosticsOpen}
        onOpenChange={setDiagnosticsOpen}
        runtime={runtime}
      />
    </div>
  );
}

export default App;
