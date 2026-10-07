import { Network, Printer, Settings2, ShieldAlert, Wrench } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import {
  AddPrinterDialog,
  InstalledClientQueuesPanel,
  TrustedServersPanel,
  useNearbyServers,
  useRecognisedClientQueues,
  useServerConnections,
  useTrustedServers,
} from "@/features/client-connection";
import { NetworkChannelPanel, useNetworkChannel } from "@/features/network-channel";
import { PrintFailuresPanel, usePrintFailures } from "@/features/print-failures";
import { SoftwareUpdatesPanel } from "@/features/updates/components/SoftwareUpdatesPanel";
import { UpdateNotice, useUpdates } from "@/features/updates";
import { PrinterSharingPanel, usePrinterSharing } from "@/features/printer-sharing";
import {
  RuntimeStatusPanel,
  SystemDiagnosticsDialog,
  useRuntimeStatus,
} from "@/features/runtime-status";
import {
  LegacyImportPanel,
  StartupPanel,
  useLegacyImport,
  useStartup,
} from "@/features/settings";
import "./App.css";

const NAV_ITEMS = [
  { id: "my-printers", label: "My printers", icon: Printer },
  { id: "share", label: "Share printers", icon: Network },
  { id: "settings", label: "Settings", icon: Settings2 },
] as const;

type PageId = (typeof NAV_ITEMS)[number]["id"];

const PAGE_CONTENT: Record<PageId, { title: string; description: string }> = {
  "my-printers": {
    title: "My printers",
    description: "Printers available to your Windows apps through ShaPrint.",
  },
  share: {
    title: "Share printers",
    description: "Choose the printers this computer makes available over the network.",
  },
  settings: {
    title: "Settings",
    description: "Set the Network Channel, startup behavior, and print services.",
  },
};

function App() {
  const [page, setPage] = useState<PageId>("my-printers");
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [addPrinterOpen, setAddPrinterOpen] = useState(false);

  const runtime = useRuntimeStatus();
  const failures = usePrintFailures();
  const startup = useStartup();
  const legacy = useLegacyImport();
  const sharing = usePrinterSharing();
  const networkChannel = useNetworkChannel();
  const connections = useServerConnections();
  const trustedServers = useTrustedServers();
  const clientQueues = useRecognisedClientQueues();
  const nearby = useNearbyServers();
  const updates = useUpdates();
  const pageContent = PAGE_CONTENT[page];

  const hasFailedService =
    runtime.status?.services.some((s) => s.state === "failed") || runtime.error !== null;
  const isTransitioning =
    runtime.status?.services.some((s) => s.state === "starting" || s.state === "stopping") ||
    runtime.busy !== null;
  const isUnreachable = runtime.unreachable;
  const clientProxyState = runtime.status?.services.find((s) => s.id === "client-proxy")?.state;
  const serverSharingState = runtime.status?.services.find((s) => s.id === "server-sharing")?.state;
  const hasRunningPath = clientProxyState === "running" || serverSharingState === "running";
  const statusLabel = isUnreachable
    ? "Runtime unavailable"
    : hasFailedService
      ? "Service needs attention"
      : isTransitioning
        ? "Service changing"
        : runtime.status === null
          ? "Checking services"
          : clientProxyState === "running" && serverSharingState === "running"
            ? "Client + server ready"
            : clientProxyState === "running"
              ? "Client printing ready"
              : serverSharingState === "running"
                ? "Server sharing on"
                : "Print paths stopped";
  const statusTone =
    isUnreachable || hasFailedService
      ? "problem"
      : isTransitioning || runtime.status === null
        ? "busy"
        : hasRunningPath
          ? "ready"
          : "idle";

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
          <p>Print from the Windows dialog you already use.</p>
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
              data-tone={statusTone}
              title="Open service diagnostics"
              aria-label={`${statusLabel}. Open service diagnostics`}
            >
              <span className={`status-dot dot-${statusTone}`} aria-hidden="true" />
              <span className="status-label">{statusLabel}</span>
              <Wrench size={13} className="status-icon" aria-hidden="true" />
            </button>
          </div>
        </header>

        <div className="workspace-content">
          <UpdateNotice
            status={updates.status}
            error={updates.error}
            checking={updates.checking}
            onCheck={() => void updates.check()}
            onRestart={() => void updates.restart()}
          />
          <PrintFailuresPanel failures={failures} />

          <section className="page-stack" aria-label={pageContent.title}>
            {page === "my-printers" ? (
              <>
                <InstalledClientQueuesPanel
                  clientQueues={clientQueues}
                  onAddPrinter={() => setAddPrinterOpen(true)}
                  onReverifyServer={(serverAddress) => {
                    setAddPrinterOpen(true);
                    void connections.reviewAddress(serverAddress);
                  }}
                />
                <TrustedServersPanel
                  trusted={trustedServers}
                  onReviewServer={(serverAddress) => {
                    setAddPrinterOpen(true);
                    void connections.reviewAddress(serverAddress);
                  }}
                />
              </>
            ) : null}

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
                <PrinterSharingPanel
                  sharing={sharing}
                  runtime={runtime}
                  networkChannel={networkChannel}
                />
              </>
            ) : null}

            {page === "settings" ? (
              <>
                <NetworkChannelPanel settings={networkChannel} />
                <StartupPanel startup={startup} />
                <SoftwareUpdatesPanel updates={updates} />
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
        onOpenChange={(nextOpen) => {
          setAddPrinterOpen(nextOpen);
          if (!nextOpen) {
            void clientQueues.refresh();
            void trustedServers.reload();
          }
        }}
        nearby={nearby}
        connections={connections}
        networkChannel={networkChannel}
        onInstalled={() => {
          void clientQueues.refresh();
        }}
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
