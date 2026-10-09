import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import * as discovery from "./api/discovery";
import * as ipc from "./api/ipc";
import * as legacyApi from "./api/legacyImport";
import type { LegacyImportReport } from "./api/legacyImport";
import * as networkChannel from "./api/networkChannel";
import * as printFailures from "./api/printFailures";
import * as serverConnections from "./api/serverConnections";
import * as startupApi from "./api/startup";
import * as updatesApi from "./api/updates";
import { CLIENT_VERSION } from "./lib/version";
import type { UpdateStatus } from "./api/updates";
import type {
  LocalPrinters,
  NearbyServers,
  PrintFailure,
  RuntimeStatus,
  ServerIdentity,
} from "./api/types";

vi.mock("./api/updates", () => ({
  getUpdateStatus: vi.fn(),
  checkForUpdates: vi.fn(),
  applyUpdateAndRestart: vi.fn(),
  onUpdateStatus: vi.fn(),
}));

vi.mock("./api/ipc", () => ({
  RUNTIME_STATUS_EVENT: "runtime://status",
  getRuntimeStatus: vi.fn(),
  startService: vi.fn(),
  stopService: vi.fn(),
  onRuntimeStatus: vi.fn(),
  listLocalPrinters: vi.fn(),
  setSharedPrinters: vi.fn(),
  getServerIdentity: vi.fn(),
}));

vi.mock("./api/networkChannel", () => ({
  getNetworkChannelStatus: vi.fn(),
  configureNetworkChannel: vi.fn(),
}));

vi.mock("./api/serverConnections", () => ({
  inspectServerConnection: vi.fn(),
  approveServerConnection: vi.fn(),
  listServerConnectionPrinters: vi.fn(),
  listTrustedServers: vi.fn(),
  probeTrustedServer: vi.fn(),
  forgetTrustedServer: vi.fn(),
  installPrinterQueue: vi.fn(),
  listRecognisedClientQueues: vi.fn(),
  openPrintersSettings: vi.fn(),
}));

vi.mock("./api/discovery", () => ({
  NEARBY_SERVERS_EVENT: "discovery://servers",
  listNearbyServers: vi.fn(),
  onNearbyServers: vi.fn(),
}));

vi.mock("./api/printFailures", () => ({
  PRINT_FAILURE_EVENT: "runtime://print-failure",
  getPrintFailures: vi.fn(),
  dismissPrintFailure: vi.fn(),
  onPrintFailure: vi.fn(),
}));

vi.mock("./api/startup", () => ({
  getStartupStatus: vi.fn(),
  setStartupEnabled: vi.fn(),
}));

vi.mock("./api/legacyImport", () => ({
  LEGACY_IMPORT_EVENT: "legacy://import",
  getLegacyImportReport: vi.fn(),
  importLegacySettings: vi.fn(),
  onLegacyImport: vi.fn(),
}));

const runtimeWith = (
  proxy: RuntimeStatus["services"][number]["state"],
  sharing: RuntimeStatus["services"][number]["state"],
): RuntimeStatus => ({
  services: [
    { id: "client-proxy", state: proxy, detail: proxy === "running" ? "running" : "" },
    { id: "server-sharing", state: sharing, detail: sharing === "running" ? "running" : "" },
  ],
});

const nearbyWith = (...names: string[]): NearbyServers => ({
  servers: names.map((name, index) => ({
    name,
    address: `192.0.2.${10 + index}:8631`,
    printers: ["Zebra"],
  })),
});

const IDENTITY: ServerIdentity = {
  fingerprint: "5F:3A:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD",
  port: 48631,
};

const printersWith = (...shared: string[]): LocalPrinters => ({
  printers: ["HP LaserJet", "Zebra", "Canon"].map((name) => ({
    name,
    shared: shared.includes(name),
  })),
});

function serviceCard(container: HTMLElement, id: string): HTMLElement {
  const card = container.querySelector(`[data-service="${id}"]`);
  if (card === null) {
    throw new Error(`no card rendered for ${id}`);
  }
  return card as HTMLElement;
}

type WorkspacePage = "My printers" | "Share printers" | "Settings";

function navigateTo(page: WorkspacePage) {
  fireEvent.click(screen.getByRole("button", { name: page }));
}

function printerRow(container: HTMLElement, name: string): HTMLElement {
  const row = container.querySelector(`[data-printer="${name}"]`);
  if (row === null) {
    throw new Error(`no row rendered for ${name}`);
  }
  return row as HTMLElement;
}

/** Waits until the sharing panel has listed the queues. */
async function sharingPanel(container: HTMLElement): Promise<HTMLElement> {
  navigateTo("Share printers");
  await waitFor(() => expect(printerRow(container, "HP LaserJet")).toBeTruthy());
  return screen.getByRole("region", { name: "Local printers" });
}

// Testing Library only cleans up automatically when Vitest globals are turned on; this suite
// queries the whole document, so earlier renders must not stay behind.
afterEach(cleanup);

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(ipc.onRuntimeStatus).mockResolvedValue(() => {});
  vi.mocked(updatesApi.getUpdateStatus).mockResolvedValue({
    current_version: "3.1.1",
    update: { state: "idle" },
    waiting_for_jobs: false,
  });
  vi.mocked(updatesApi.onUpdateStatus).mockResolvedValue(() => {});
  vi.mocked(updatesApi.checkForUpdates).mockResolvedValue({
    current_version: "3.1.1",
    update: { state: "idle" },
    waiting_for_jobs: false,
  });
  vi.mocked(updatesApi.applyUpdateAndRestart).mockResolvedValue({
    current_version: "3.1.1",
    update: { state: "ready_to_restart", version: "3.2.0", restart_requested: true },
    waiting_for_jobs: false,
  });
  vi.mocked(ipc.getRuntimeStatus).mockResolvedValue(runtimeWith("running", "stopped"));
  vi.mocked(ipc.listLocalPrinters).mockResolvedValue(printersWith());
  vi.mocked(ipc.getServerIdentity).mockResolvedValue(IDENTITY);
  vi.mocked(serverConnections.listRecognisedClientQueues).mockResolvedValue([]);
  vi.mocked(serverConnections.listTrustedServers).mockResolvedValue([]);
  vi.mocked(serverConnections.forgetTrustedServer).mockResolvedValue();
  vi.mocked(networkChannel.getNetworkChannelStatus).mockResolvedValue(false);
  vi.mocked(discovery.listNearbyServers).mockResolvedValue({ servers: [] });
  vi.mocked(discovery.onNearbyServers).mockResolvedValue(() => {});
  vi.mocked(printFailures.getPrintFailures).mockResolvedValue(null);
  vi.mocked(printFailures.onPrintFailure).mockResolvedValue(() => {});
  vi.mocked(startupApi.getStartupStatus).mockResolvedValue({
    supported: true,
    enabled: true,
    command: '"C:\\Program Files\\ShaPrint\\shaprint-desktop.exe" --background',
  });
  vi.mocked(legacyApi.getLegacyImportReport).mockResolvedValue({
    found: false,
    channel: "absent",
    channel_note:
      "The previous ShaPrint app had no Network Channel of its own. Set one here before clients can print.",
    settings: [],
    queues_need_reselection: false,
  });
  vi.mocked(legacyApi.onLegacyImport).mockResolvedValue(() => {});
});

/** A computer that had the previous .NET app installed. */
const LEGACY_REPORT: LegacyImportReport = {
  found: true,
  channel: "imported",
  channel_note: "Your Network Channel was imported from the previous ShaPrint app.",
  settings: [
    {
      key: "auto-update",
      label: "Automatic updates",
      reason: "This app does not update itself yet.",
    },
    {
      key: "client-queues",
      label: "Installed client printers",
      reason:
        "Printer queues are installed again from this app; the previous app's queues are not activated.",
    },
  ],
  queues_need_reselection: true,
};

const FAILURE: PrintFailure = {
  path: "server-submission",
  code: "queue-unavailable",
  message:
    "The job for 'Office Printer' could not be submitted to the Windows printer queue.",
  recovery:
    "Check that the printer is switched on and reachable from this computer, then print again.",
  observed_at_ms: Date.UTC(2026, 0, 2, 3, 4, 5),
};

describe("updater", () => {
  it("shows a ready update banner and queues restart while jobs drain", async () => {
    let publish: ((status: UpdateStatus) => void) | undefined;
    vi.mocked(updatesApi.onUpdateStatus).mockImplementation(async (handler) => {
      publish = handler;
      return () => {};
    });
    render(<App />);
    await waitFor(() => expect(publish).toBeDefined());

    publish?.({
      current_version: "3.1.1",
      update: { state: "ready_to_restart", version: "3.2.0", restart_requested: false },
      waiting_for_jobs: true,
    });

    expect(await screen.findByText("ShaPrint 3.2.0 is ready")).toBeTruthy();
    expect(screen.getByText(/restart is queued.*printing and spooler cleanup finish/i)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Restart queued" }));
    await waitFor(() => expect(updatesApi.applyUpdateAndRestart).toHaveBeenCalledTimes(1));
  });

  it("reports manual update-check failures in plain language", async () => {
    vi.mocked(updatesApi.checkForUpdates).mockRejectedValue({
      message: "GitHub Releases could not be reached. Check your internet connection and try again.",
    });
    render(<App />);
    navigateTo("Settings");

    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));

    const alerts = await screen.findAllByRole("alert");
    expect(alerts.some((el) => el.textContent?.includes("GitHub Releases could not be reached"))).toBe(true);
  });

  it("clears a previous update error when a later status event succeeds", async () => {
    let publish: ((status: UpdateStatus) => void) | undefined;
    vi.mocked(updatesApi.onUpdateStatus).mockImplementation(async (handler) => {
      publish = handler;
      return () => {};
    });
    render(<App />);
    await waitFor(() => expect(publish).toBeDefined());

    await act(async () => publish?.({
      current_version: "3.1.1",
      update: { state: "failed", message: "Check failed." },
      waiting_for_jobs: false,
    }));
    expect((await screen.findByRole("alert")).textContent).toBe("Check failed.");

    await act(async () => publish?.({
      current_version: "3.1.1",
      update: { state: "idle" },
      waiting_for_jobs: false,
    }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("displays prominent nightly badges and top bar when running on a nightly build", async () => {
    vi.mocked(updatesApi.getUpdateStatus).mockResolvedValue({
      current_version: "3.4.0-nightly.1",
      update: { state: "idle" },
      waiting_for_jobs: false,
    });
    render(<App />);

    // Top banner and sidebar indicator
    expect(await screen.findByText(/ShaPrint Nightly/i)).toBeTruthy();
    expect(screen.getByText("Pre-release Test Build")).toBeTruthy();
    expect(screen.getByText("Nightly")).toBeTruthy();

    // Settings page software updates panel
    navigateTo("Settings");
    expect(await screen.findByText("Nightly Channel")).toBeTruthy();
    expect(screen.getByText("Nightly Channel Active")).toBeTruthy();
  });

  it("omits nightly branding and displays stable indicator when on a stable build", async () => {
    vi.mocked(updatesApi.getUpdateStatus).mockResolvedValue({
      current_version: "3.4.0",
      update: { state: "idle" },
      waiting_for_jobs: false,
    });
    render(<App />);

    expect(screen.queryByText("Pre-release Test Build")).toBeNull();
    expect(screen.queryByText("Nightly")).toBeNull();

    navigateTo("Settings");
    expect(await screen.findByText("Stable")).toBeTruthy();
    expect(screen.queryByText("Nightly Channel Active")).toBeNull();
  });
});

describe("workspace navigation", () => {
  it("starts on My printers and keeps Share printers and Settings in their own sections", async () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "My printers", level: 1 })).toBeTruthy();
    expect(screen.getByRole("button", { name: "My printers" }).getAttribute("aria-current")).toBe("page");
    expect(screen.queryByRole("region", { name: "Local printers" })).toBeNull();

    navigateTo("Share printers");
    expect(await screen.findByRole("heading", { name: "Share printers", level: 1 })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Local printers" })).toBeTruthy();

    navigateTo("Settings");
    expect(await screen.findByRole("heading", { name: "Settings", level: 1 })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Print authorization" })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Software updates" })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Startup" })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Print services" })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Previous ShaPrint app" })).toBeTruthy();
  });

  it("navigates from status pill or problem condition directly to service diagnostics dialog", async () => {
    vi.mocked(ipc.getRuntimeStatus).mockResolvedValue(runtimeWith("failed", "stopped"));
    render(<App />);

    const statusPill = await screen.findByRole("button", {
      name: /Service needs attention\. Open service diagnostics/i,
    });
    expect(statusPill).toBeTruthy();
    fireEvent.click(statusPill);

    // Diagnostics dialog opens with detailed technical service controls
    const dialog = await screen.findByRole("dialog", { name: "Print service status" });
    expect(dialog).toBeTruthy();
    expect(within(dialog).getByRole("heading", { name: "Client proxy" })).toBeTruthy();
    expect(within(dialog).getByRole("heading", { name: "Server sharing" })).toBeTruthy();
  });
});

describe("workspace service status", () => {
  it("names the active client print path instead of implying every service is running", async () => {
    render(<App />);

    expect(
      await screen.findByRole("button", {
        name: "Client printing ready. Open service diagnostics",
      }),
    ).toBeTruthy();
  });

  it("reports stopped print paths when neither service is running", async () => {
    vi.mocked(ipc.getRuntimeStatus).mockResolvedValue(runtimeWith("stopped", "stopped"));
    render(<App />);

    expect(
      await screen.findByRole("button", {
        name: "Print paths stopped. Open service diagnostics",
      }),
    ).toBeTruthy();
  });
});

describe("guided printer setup", () => {
  it("renders recognised client queues from Windows and allows refresh and retry", async () => {
    vi.mocked(serverConnections.listRecognisedClientQueues).mockResolvedValueOnce([
      {
        queue_name: "Office Laser (ShaPrint 10.0.0.5-8631)",
        server_address: "10.0.0.5:8631",
        printer_name: "Office Laser",
      },
      {
        queue_name: "Office Laser (ShaPrint 192.168.1.50-8631)",
        server_address: "192.168.1.50:8631",
        printer_name: "Office Laser",
      },
    ]);

    render(<App />);

    // Both queues, including same printer name on different servers, are listed on default My printers screen
    await waitFor(() => {
      expect(
        screen.getByText("Office Laser (ShaPrint 10.0.0.5-8631)"),
      ).toBeTruthy();
      expect(
        screen.getByText("Office Laser (ShaPrint 192.168.1.50-8631)"),
      ).toBeTruthy();
    });

    // Refresh button fetches current state again
    vi.mocked(serverConnections.listRecognisedClientQueues).mockResolvedValueOnce([]);
    fireEvent.click(screen.getByRole("button", { name: "Refresh installed queues" }));

    await waitFor(() => {
      expect(
        screen.getByText(
          "No ShaPrint client queues installed in Windows.",
        ),
      ).toBeTruthy();
    });

    // Error and retry state
    vi.mocked(serverConnections.listRecognisedClientQueues).mockRejectedValueOnce(
      new Error("Spooler service unavailable"),
    );
    fireEvent.click(screen.getByRole("button", { name: "Refresh installed queues" }));

    await waitFor(() => {
      expect(
        screen.getByText("Spooler service unavailable"),
      ).toBeTruthy();
      expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
    });
  });

  it("opens Windows settings or explains manual steps without offering in-app delete button", async () => {
    vi.mocked(serverConnections.listRecognisedClientQueues).mockResolvedValue([
      {
        queue_name: "Office Laser (ShaPrint 10.0.0.5-8631)",
        server_address: "10.0.0.5:8631",
        printer_name: "Office Laser",
      },
    ]);
    vi.mocked(serverConnections.openPrintersSettings).mockResolvedValue();

    render(<App />);

    expect(
      await screen.findByText("Office Laser (ShaPrint 10.0.0.5-8631)"),
    ).toBeTruthy();

    // No in-app delete button
    expect(screen.queryByRole("button", { name: /delete/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /remove/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /uninstall/i })).toBeNull();

    // Manage in Windows action calls desktop shell
    const manageButtons = screen.getAllByRole("button", { name: /Manage in Windows/i });
    expect(manageButtons.length).toBeGreaterThan(0);
    fireEvent.click(manageButtons[0]);

    await waitFor(() => {
      expect(serverConnections.openPrintersSettings).toHaveBeenCalled();
    });

    // When opening settings fails, explains manual navigation and provides queue name copy
    vi.mocked(serverConnections.openPrintersSettings).mockRejectedValueOnce(
      new Error("Shell execution failed"),
    );
    fireEvent.click(manageButtons[0]);

    expect(
      await screen.findByText(/Could not open Windows Settings automatically/i),
    ).toBeTruthy();
    expect(
      screen.getByText(/Open Windows Settings > Bluetooth & devices > Printers & scanners/i),
    ).toBeTruthy();

    // Queue name copy button is available
    const copyBtn = screen.getByRole("button", {
      name: "Copy queue name Office Laser (ShaPrint 10.0.0.5-8631)",
    });
    expect(copyBtn).toBeTruthy();
  });

  it("shows evidence-based Reverify identity action when an installed queue's server identity has changed", async () => {
    vi.mocked(serverConnections.listRecognisedClientQueues).mockResolvedValue([
      {
        queue_name: "Office Laser (ShaPrint 10.0.0.5-8631)",
        server_address: "10.0.0.5:8631",
        printer_name: "Office Laser",
      },
    ]);
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue({
      address: "10.0.0.5:8631",
      current_fingerprint: "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99",
      previous_fingerprint: "11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF",
      trusted: false,
    });

    render(<App />);

    // Queue remains listed with targeted evidence warning
    expect(
      await screen.findByText("Office Laser (ShaPrint 10.0.0.5-8631)"),
    ).toBeTruthy();
    expect(
      await screen.findByText("Server identity changed. Printing is blocked until reverified."),
    ).toBeTruthy();

    // Does not display speculative Online or Offline badges
    expect(screen.queryByText(/^Online$/i)).toBeNull();
    expect(screen.queryByText(/^Offline$/i)).toBeNull();

    // Reverify identity button opens the guided trust verification flow
    const reverifyBtn = screen.getByRole("button", {
      name: "Reverify identity for 10.0.0.5:8631",
    });
    expect(reverifyBtn).toBeTruthy();
    fireEvent.click(reverifyBtn);

    // Guided setup dialog opens directly to fingerprint verification step
    expect(await screen.findByRole("dialog", { name: "Check the server identity" })).toBeTruthy();
  });

  it("offers a single guided Add printer entry in My printers workspace without duplicate inline controls", async () => {
    render(<App />);

    // The single guided entry action is available right on the default My printers screen
    expect(screen.getByRole("button", { name: "Add printer" })).toBeTruthy();

    // The duplicate inline inspection and install path is no longer present
    expect(screen.queryByLabelText("Server address")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Add by address" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Inspect certificate" })).toBeNull();
  });

  it("keeps trusted servers visible with live status, refresh, printers, and forget actions", async () => {
    const onlineAddress = "print-east.example:48631";
    const offlineAddress = "print-west.example:48631";
    const changedAddress = "print-old.example:48631";
    const forgottenAddress = "print-retired.example:48631";
    const changedFingerprint =
      "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77";
    vi.mocked(serverConnections.listTrustedServers).mockResolvedValue([
      { address: onlineAddress, fingerprint: IDENTITY.fingerprint },
      { address: offlineAddress, fingerprint: IDENTITY.fingerprint },
      { address: changedAddress, fingerprint: IDENTITY.fingerprint },
      { address: forgottenAddress, fingerprint: IDENTITY.fingerprint },
    ]);
    let offlineProbes = 0;
    vi.mocked(serverConnections.probeTrustedServer).mockImplementation(async (address) => {
      if (address === onlineAddress) {
        return {
          address,
          status: "online",
          approved_fingerprint: IDENTITY.fingerprint,
          current_fingerprint: IDENTITY.fingerprint,
          printers: ["Zebra"],
        };
      }
      if (address === offlineAddress) {
        offlineProbes += 1;
        if (offlineProbes > 1) {
          return {
            address,
            status: "online",
            approved_fingerprint: IDENTITY.fingerprint,
            current_fingerprint: IDENTITY.fingerprint,
            printers: ["Receipt printer"],
          };
        }
      }
      if (address === changedAddress) {
        return {
          address,
          status: "identity_changed",
          approved_fingerprint: IDENTITY.fingerprint,
          current_fingerprint: changedFingerprint,
          printers: [],
        };
      }
      return {
        address,
        status: "offline",
        approved_fingerprint: IDENTITY.fingerprint,
        current_fingerprint: null,
        printers: [],
      };
    });

    render(<App />);

    expect(await screen.findByText(onlineAddress)).toBeTruthy();
    expect(screen.getByText(offlineAddress)).toBeTruthy();
    expect(screen.getByText(changedAddress)).toBeTruthy();
    expect(screen.getByText(forgottenAddress)).toBeTruthy();
    expect(screen.getByText("Online")).toBeTruthy();
    expect(screen.getAllByText("Offline").length).toBe(2);
    expect(screen.getByText("Identity changed")).toBeTruthy();
    expect(screen.getByText(/Shared printers: Zebra/)).toBeTruthy();
    expect(screen.getByText(changedFingerprint)).toBeTruthy();
    expect(
      screen.getByRole("button", { name: `Review shared printers for ${onlineAddress}` }),
    ).toBeTruthy();
    await waitFor(() => expect(serverConnections.probeTrustedServer).toHaveBeenCalledTimes(4));

    fireEvent.click(screen.getByRole("button", { name: `Refresh status for ${offlineAddress}` }));
    await waitFor(() => expect(serverConnections.probeTrustedServer).toHaveBeenCalledTimes(5));
    expect(serverConnections.probeTrustedServer).toHaveBeenLastCalledWith(offlineAddress);
    expect(await screen.findByText(/Shared printers: Receipt printer/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: `Forget ${forgottenAddress}` }));
    await waitFor(() => {
      expect(serverConnections.forgetTrustedServer).toHaveBeenCalledWith(forgottenAddress);
      expect(screen.queryByText(forgottenAddress)).toBeNull();
    });
    expect(screen.getByText(offlineAddress)).toBeTruthy();
    expect(screen.getByText(onlineAddress)).toBeTruthy();
  });

  it("labels advertised printer names as unverified before approval in nearby panel and guided dialog", async () => {
    vi.mocked(discovery.listNearbyServers).mockResolvedValue({
      servers: [
        {
          name: "DESKTOP-ABC",
          address: "192.0.2.10:8631",
          printers: ["Zebra", "Receipt"],
          version: "3.1.3",
        },
      ],
    });

    render(<App />);

    // Open guided setup from My printers
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));
    const dialog = await screen.findByRole("dialog", { name: "Find a server" });
    expect(within(dialog).getByText(/Advertised \(unverified\): Zebra, Receipt/)).toBeTruthy();
  });

  it("completes guided add printer flow from nearby server with observable IPC ordering", async () => {
    vi.mocked(networkChannel.getNetworkChannelStatus).mockResolvedValue(true);
    vi.mocked(discovery.listNearbyServers).mockResolvedValue({
      servers: [
        {
          name: "DESKTOP-ABC",
          address: "192.0.2.10:8631",
          printers: ["Office Laser"],
          version: "3.1.3",
        },
      ],
    });
    const review = {
      address: "192.0.2.10:8631",
      current_fingerprint: IDENTITY.fingerprint,
      previous_fingerprint: null,
      trusted: false,
    };
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue(review);
    vi.mocked(serverConnections.approveServerConnection).mockResolvedValue({
      ...review,
      trusted: true,
    });
    vi.mocked(serverConnections.listServerConnectionPrinters).mockResolvedValue({
      address: review.address,
      printers: ["Office Laser"],
    });
    vi.mocked(serverConnections.installPrinterQueue).mockResolvedValue({
      queue_name: "Office Laser (ShaPrint 192.0.2.10-8631)",
      server_address: review.address,
      printer_name: "Office Laser",
      uri: "ipp://127.0.0.1:8632/ipp/print/192.0.2.10%3A8631/Office%20Laser",
    });

    render(<App />);

    // Click Add printer from My printers workspace
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    // Select nearby server from wizard
    const dialog = await screen.findByRole("dialog", { name: "Find a server" });
    const serverOption = within(dialog).getByText("DESKTOP-ABC").closest(".wizard-server-option") as HTMLElement;
    fireEvent.click(within(serverOption).getByRole("button", { name: "Review identity" }));

    // Opens dialog, inspects certificate, reaches verification step
    expect(await screen.findByRole("dialog", { name: "Check the server identity" })).toBeTruthy();
    expect(serverConnections.inspectServerConnection).toHaveBeenCalledWith("192.0.2.10:8631");
    // Authoritative shared-printer queries are blocked until server identity is approved
    expect(serverConnections.listServerConnectionPrinters).not.toHaveBeenCalled();

    // Approve fingerprint
    fireEvent.click(screen.getByRole("button", { name: "Approve fingerprint" }));

    // Now queries printers and advances to printers step
    await waitFor(() => {
      expect(serverConnections.approveServerConnection).toHaveBeenCalledWith(
        review.address,
        review.current_fingerprint,
      );
      expect(serverConnections.listServerConnectionPrinters).toHaveBeenCalledWith(review.address);
    });

    expect(await screen.findByText("Office Laser")).toBeTruthy();

    // Install printer
    fireEvent.click(screen.getByRole("button", { name: "Install" }));

    await waitFor(() => {
      expect(serverConnections.installPrinterQueue).toHaveBeenCalledWith(
        review.address,
        "Office Laser",
      );
    });

    // Success screen names the Windows queue and explains printing via ordinary Windows print dialog
    expect(await screen.findByRole("heading", { name: "Printer installed" })).toBeTruthy();
    expect(screen.getByText("Office Laser (ShaPrint 192.0.2.10-8631)")).toBeTruthy();
    expect(screen.getByText(/Choose this printer from any Windows print dialog/)).toBeTruthy();

    // Verifies listRecognisedClientQueues was automatically refreshed upon installation
    expect(serverConnections.listRecognisedClientQueues).toHaveBeenCalled();

    // When the user clicks Done and closes the dialog, the newly installed queue is visible in My printers
    vi.mocked(serverConnections.listRecognisedClientQueues).mockResolvedValue([
      {
        queue_name: "Office Laser (ShaPrint 192.0.2.10-8631)",
        server_address: review.address,
        printer_name: "Office Laser",
      },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Done" }));

    expect(await screen.findByText("Office Laser (ShaPrint 192.0.2.10-8631)")).toBeTruthy();
  });

  it("skips repeat approval when server identity matches saved approval (trusted repeat run)", async () => {
    const review = {
      address: "192.0.2.10:8631",
      current_fingerprint: IDENTITY.fingerprint,
      previous_fingerprint: null,
      trusted: true,
    };
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue(review);
    vi.mocked(serverConnections.listServerConnectionPrinters).mockResolvedValue({
      address: review.address,
      printers: ["Office Laser"],
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    const dialog = await screen.findByRole("dialog", { name: "Find a server" });
    fireEvent.change(within(dialog).getByLabelText("Or enter a server address"), {
      target: { value: "192.0.2.10:8631" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review identity" }));

    // Automatically jumps to printers step and queries printers without asking for approval
    expect(await screen.findByRole("heading", { name: "Choose a printer" })).toBeTruthy();
    expect(serverConnections.approveServerConnection).not.toHaveBeenCalled();
    expect(serverConnections.listServerConnectionPrinters).toHaveBeenCalledWith(review.address);
    expect(await screen.findByText("Office Laser")).toBeTruthy();
  });

  it("blocks printer queries and requires explicit reapproval when server identity has changed", async () => {
    const review = {
      address: "192.0.2.10:8631",
      current_fingerprint:
        "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77",
      previous_fingerprint: IDENTITY.fingerprint,
      trusted: false,
    };
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue(review);
    vi.mocked(serverConnections.approveServerConnection).mockResolvedValue({
      ...review,
      trusted: true,
    });
    vi.mocked(serverConnections.listServerConnectionPrinters).mockResolvedValue({
      address: review.address,
      printers: ["Office Laser"],
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    const dialog = await screen.findByRole("dialog", { name: "Find a server" });
    fireEvent.change(within(dialog).getByLabelText("Or enter a server address"), {
      target: { value: "192.0.2.10:8631" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review identity" }));

    // Shows changed warning
    expect(await screen.findByText("Server identity has changed.")).toBeTruthy();
    const reapproveBtn = screen.getByRole("button", { name: "Reapprove fingerprint" });
    expect(reapproveBtn).toBeTruthy();

    // Authoritative printer query is blocked before reapproval
    expect(serverConnections.listServerConnectionPrinters).not.toHaveBeenCalled();

    // User explicitly reapproves
    fireEvent.click(reapproveBtn);

    await waitFor(() => {
      expect(serverConnections.approveServerConnection).toHaveBeenCalledWith(
        review.address,
        review.current_fingerprint,
      );
      expect(serverConnections.listServerConnectionPrinters).toHaveBeenCalledWith(review.address);
    });
    expect(await screen.findByText("Office Laser")).toBeTruthy();
  });

  it("reflects conditional Network Channel step in wizard progress and requires channel before install", async () => {
    // When channel is not configured
    vi.mocked(networkChannel.getNetworkChannelStatus).mockResolvedValue(false);
    const review = {
      address: "192.0.2.10:8631",
      current_fingerprint: IDENTITY.fingerprint,
      previous_fingerprint: null,
      trusted: true,
    };
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue(review);
    vi.mocked(serverConnections.listServerConnectionPrinters).mockResolvedValue({
      address: review.address,
      printers: ["Office Laser"],
    });
    vi.mocked(networkChannel.configureNetworkChannel).mockResolvedValue(true);
    vi.mocked(serverConnections.installPrinterQueue).mockResolvedValue({
      queue_name: "Office Laser (ShaPrint 192.0.2.10-8631)",
      server_address: review.address,
      printer_name: "Office Laser",
      uri: "ipp://127.0.0.1:8632/ipp/print/192.0.2.10%3A8631/Office%20Laser",
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    const dialog = await screen.findByRole("dialog", { name: "Find a server" });

    // Step list includes Network Channel when not configured
    const stepList = within(dialog).getByRole("list", { name: "Printer setup steps" });
    expect(within(stepList).getByText("Network Channel")).toBeTruthy();

    // Proceed to printers
    fireEvent.change(within(dialog).getByLabelText("Or enter a server address"), {
      target: { value: "192.0.2.10:8631" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review identity" }));

    expect(await screen.findByRole("heading", { name: "Choose a printer" })).toBeTruthy();

    // Click install -> prompts for Network Channel
    fireEvent.click(screen.getByRole("button", { name: "Install" }));

    expect(await screen.findByRole("heading", { name: "Set the Network Channel" })).toBeTruthy();
    expect(within(stepList).getByText("Network Channel").closest("li")?.getAttribute("aria-current")).toBe("step");

    // Enter channel and submit
    fireEvent.change(screen.getByLabelText("Network Channel"), {
      target: { value: "secret-channel" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save Network Channel and install" }));

    await waitFor(() => {
      expect(networkChannel.configureNetworkChannel).toHaveBeenCalledWith("secret-channel");
      expect(serverConnections.installPrinterQueue).toHaveBeenCalledWith(
        review.address,
        "Office Laser",
      );
    });

    expect(await screen.findByRole("heading", { name: "Printer installed" })).toBeTruthy();
  });

  it("resets selection, progress, and error state when reopening the journey after success, cancellation, or failure", async () => {
    vi.mocked(networkChannel.getNetworkChannelStatus).mockResolvedValue(true);
    const review = {
      address: "192.0.2.10:8631",
      current_fingerprint: IDENTITY.fingerprint,
      previous_fingerprint: null,
      trusted: true,
    };
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue(review);
    vi.mocked(serverConnections.listServerConnectionPrinters).mockResolvedValue({
      address: review.address,
      printers: ["Office Laser"],
    });
    vi.mocked(serverConnections.installPrinterQueue).mockResolvedValue({
      queue_name: "Office Laser (ShaPrint 192.0.2.10-8631)",
      server_address: review.address,
      printer_name: "Office Laser",
      uri: "ipp://127.0.0.1:8632/ipp/print/192.0.2.10%3A8631/Office%20Laser",
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    let dialog = await screen.findByRole("dialog", { name: "Find a server" });
    fireEvent.change(within(dialog).getByLabelText("Or enter a server address"), {
      target: { value: "192.0.2.10:8631" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review identity" }));

    expect(await screen.findByRole("heading", { name: "Choose a printer" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Install" }));
    expect(await screen.findByRole("heading", { name: "Printer installed" })).toBeTruthy();

    // Click Done to close
    fireEvent.click(screen.getByRole("button", { name: "Done" }));

    // Reopen Add printer
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    dialog = await screen.findByRole("dialog", { name: "Find a server" });
    // Starts fresh at step 1 with empty input and no stale installed queue
    expect(within(dialog).getByRole("heading", { name: "Find a server" })).toBeTruthy();
    expect((within(dialog).getByLabelText("Or enter a server address") as HTMLInputElement).value).toBe("");
    expect(screen.queryByText("Office Laser (ShaPrint 192.0.2.10-8631)")).toBeNull();
  });

  it("supports keyboard navigation and narrow desktop window layout in guided setup", async () => {
    // Simulate narrow window
    window.innerWidth = 480;

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    const dialog = await screen.findByRole("dialog", { name: "Find a server" });
    expect(dialog).toBeTruthy();

    // Dialog has proper accessible attributes
    expect(dialog.getAttribute("aria-labelledby")).toBe("add-printer-title");
    expect(dialog.getAttribute("aria-describedby")).toBe("add-printer-description");

    // Close with Escape key
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });

    // Reset window width
    window.innerWidth = 1024;
  });
});

describe("runtime status panel", () => {
  it("shows the live state of the proxy and sharing services", async () => {
    const { container } = render(<App />);
    navigateTo("Settings");

    await waitFor(() => expect(container.querySelector('[data-service="client-proxy"]')).not.toBeNull());

    expect(serviceCard(container, "client-proxy").dataset.state).toBe("running");
    expect(serviceCard(container, "server-sharing").dataset.state).toBe("stopped");
    expect(screen.getByText("Client proxy")).toBeTruthy();
    expect(screen.getByText("Server sharing")).toBeTruthy();
  });

  it("follows status events published by the shell", async () => {
    let publish: ((status: RuntimeStatus) => void) | undefined;
    vi.mocked(ipc.onRuntimeStatus).mockImplementation(async (handler) => {
      publish = handler;
      return () => {};
    });
    const { container } = render(<App />);
    navigateTo("Settings");

    await waitFor(() => expect(publish).toBeDefined());
    publish?.(runtimeWith("failed", "running"));

    await waitFor(() => expect(serviceCard(container, "client-proxy").dataset.state).toBe("failed"));
    expect(serviceCard(container, "server-sharing").dataset.state).toBe("running");

    // A failed service can be retried; a running one can only be stopped.
    const failed = serviceCard(container, "client-proxy");
    expect(within(failed).getByRole("button", { name: "Start" }).hasAttribute("disabled")).toBe(false);
    expect(within(failed).getByRole("button", { name: "Stop" }).hasAttribute("disabled")).toBe(true);
    const running = serviceCard(container, "server-sharing");
    expect(within(running).getByRole("button", { name: "Start" }).hasAttribute("disabled")).toBe(true);
    expect(within(running).getByRole("button", { name: "Stop" }).hasAttribute("disabled")).toBe(false);
  });

  it("renders the status returned by a service action", async () => {
    vi.mocked(ipc.startService).mockResolvedValue(runtimeWith("running", "running"));
    const { container } = render(<App />);
    navigateTo("Settings");

    await waitFor(() => expect(container.querySelector('[data-service="server-sharing"]')).not.toBeNull());
    const sharing = serviceCard(container, "server-sharing");
    fireEvent.click(within(sharing).getByRole("button", { name: "Start" }));

    await waitFor(() => expect(sharing.dataset.state).toBe("running"));
    expect(ipc.startService).toHaveBeenCalledWith("server-sharing");
  });

  it("explains denied Client access and offers Start again", async () => {
    vi.mocked(ipc.startService)
      .mockRejectedValueOnce({ code: "unsupported", message: "Clients cannot connect through the Windows firewall: administrator permission was not granted. Approve the Windows prompt, then try again." })
      .mockResolvedValueOnce(runtimeWith("running", "running"));
    const { container } = render(<App />);
    navigateTo("Settings");
    const server = await screen.findByRole("region", { name: "Print services" });
    const sharing = serviceCard(container, "server-sharing");

    fireEvent.click(within(sharing).getByRole("button", { name: "Start" }));
    const alert = await within(server).findByRole("alert");
    expect(alert.textContent).toContain("Clients cannot connect");
    expect(sharing.dataset.state).toBe("stopped");

    fireEvent.click(within(sharing).getByRole("button", { name: "Start" }));
    await waitFor(() => expect(sharing.dataset.state).toBe("running"));
    expect(ipc.startService).toHaveBeenCalledTimes(2);
    expect(within(server).queryByRole("alert")).toBeNull();
  });

  it("shows the stable error code when a lifecycle action is rejected", async () => {
    vi.mocked(ipc.getRuntimeStatus).mockResolvedValue(runtimeWith("stopped", "stopped"));
    vi.mocked(ipc.startService).mockRejectedValue({
      code: "invalid-state",
      message: "select at least one printer to share before starting",
    });
    const { container } = render(<App />);
    navigateTo("Settings");

    await waitFor(() => expect(container.querySelector('[data-service="client-proxy"]')).not.toBeNull());
    const proxy = serviceCard(container, "client-proxy");
    fireEvent.click(within(proxy).getByRole("button", { name: "Start" }));

    const panel = screen.getByRole("region", { name: "Print services" });
    const alert = await within(panel).findByRole("alert");
    expect(alert.textContent).toContain("invalid-state");
    expect(alert.textContent).toContain("select at least one printer to share before starting");
    // The panel re-reads the status after a rejection so it cannot show a stale state.
    await waitFor(() => expect(ipc.getRuntimeStatus).toHaveBeenCalledTimes(2));
  });

  it("explains when the shell runtime cannot be reached", async () => {
    vi.mocked(ipc.getRuntimeStatus).mockRejectedValue(new Error("ipc unavailable"));
    vi.mocked(ipc.listLocalPrinters).mockRejectedValue(new Error("ipc unavailable"));
    vi.mocked(ipc.getServerIdentity).mockRejectedValue(new Error("ipc unavailable"));
    render(<App />);
    navigateTo("Settings");

    const hint = await screen.findByText(/runtime is not reachable/i);
    expect(hint).toBeTruthy();
  });

  it("names the recovery action while a print path is not doing its job", async () => {
    let publish: ((status: RuntimeStatus) => void) | undefined;
    vi.mocked(ipc.onRuntimeStatus).mockImplementation(async (handler) => {
      publish = handler;
      return () => {};
    });
    const { container } = render(<App />);
    navigateTo("Settings");

    await waitFor(() => expect(publish).toBeDefined());

    // The client proxy is the always-on print path: while it is stopped, its card says what to
    // start. Server sharing is off by default, so a stopped sharing service is not reported.
    publish?.(runtimeWith("stopped", "stopped"));
    await waitFor(() =>
      expect(serviceCard(container, "client-proxy").textContent).toContain("Printing affected"),
    );
    expect(serviceCard(container, "client-proxy").textContent).toContain("Start the client proxy");
    expect(serviceCard(container, "server-sharing").textContent).not.toContain("Printing affected");

    // A failure is worth reporting wherever it happens.
    publish?.(runtimeWith("running", "failed"));
    await waitFor(() =>
      expect(serviceCard(container, "server-sharing").textContent).toContain("Printing affected"),
    );
    expect(serviceCard(container, "server-sharing").textContent).toContain("use Start to try again");
    expect(serviceCard(container, "client-proxy").textContent).not.toContain("Printing affected");
  });
});

describe("print problems panel", () => {
  it("shows the stable code, the problem, and the next step", async () => {
    vi.mocked(printFailures.getPrintFailures).mockResolvedValue(FAILURE);
    render(<App />);

    const panel = await screen.findByRole("region", { name: "Print problems" });
    expect(await within(panel).findByText(FAILURE.message)).toBeTruthy();
    expect(within(panel).getByText("queue-unavailable")).toBeTruthy();
    expect(within(panel).getByText(FAILURE.recovery)).toBeTruthy();
    expect(within(panel).getByText("Submitting the job to the printer queue")).toBeTruthy();
  });

  it("follows the failures the shell publishes", async () => {
    let publish: ((failure: PrintFailure | null) => void) | undefined;
    vi.mocked(printFailures.onPrintFailure).mockImplementation(async (handler) => {
      publish = handler;
      return () => {};
    });
    render(<App />);

    const panel = await screen.findByRole("region", { name: "Print problems" });
    await waitFor(() => expect(publish).toBeDefined());
    publish?.({ ...FAILURE, path: "client-forwarding", code: "server-unavailable" });

    expect(await within(panel).findByText("server-unavailable")).toBeTruthy();
    expect(within(panel).getByText("Sending the job to the server")).toBeTruthy();
  });

  it("does not claim the print path is clear when failures cannot be read", async () => {
    vi.mocked(printFailures.getPrintFailures).mockRejectedValue({
      code: "internal",
      message: "print failures unavailable",
    });
    render(<App />);

    const panel = await screen.findByRole("region", { name: "Print problems" });
    expect(await within(panel).findByRole("alert")).toBeTruthy();
    expect(within(panel).queryByText(/No recent print failures/i)).toBeNull();
  });

  it("dismisses a failure and returns to the calm state", async () => {
    vi.mocked(printFailures.getPrintFailures).mockResolvedValue(FAILURE);
    vi.mocked(printFailures.dismissPrintFailure).mockResolvedValue(null);
    render(<App />);

    const panel = await screen.findByRole("region", { name: "Print problems" });
    fireEvent.click(within(panel).getByRole("button", { name: "Dismiss" }));

    await waitFor(() => expect(printFailures.dismissPrintFailure).toHaveBeenCalledTimes(1));
    expect(await within(panel).findByText(/No recent print failures/i)).toBeTruthy();
    expect(within(panel).queryByText(FAILURE.message)).toBeNull();
  });

  it("says nothing is wrong before a job fails", async () => {
    render(<App />);

    const panel = await screen.findByRole("region", { name: "Print problems" });
    expect(within(panel).getByText(/No recent print failures/i)).toBeTruthy();
    expect(within(panel).queryByRole("button", { name: "Dismiss" })).toBeNull();
  });
});

describe("shared printers panel", () => {
  it("lists the local queues and shows which of them are shared", async () => {
    vi.mocked(ipc.listLocalPrinters).mockResolvedValue(printersWith("HP LaserJet", "Canon"));
    const { container } = render(<App />);

    await sharingPanel(container);

    expect(printerRow(container, "HP LaserJet").dataset.shared).toBe("true");
    expect(printerRow(container, "Zebra").dataset.shared).toBe("false");
    expect(printerRow(container, "Canon").dataset.shared).toBe("true");
    expect(within(printerRow(container, "Zebra")).getByText("Not shared")).toBeTruthy();
    expect(screen.getByText(/2 of 3 shared/)).toBeTruthy();
  });

  it("explains when a printer search has no matches", async () => {
    vi.mocked(ipc.listLocalPrinters).mockResolvedValue({
      printers: Array.from({ length: 6 }, (_, index) => ({
        name: `Office printer ${index + 1}`,
        shared: false,
      })),
    });
    render(<App />);
    navigateTo("Share printers");

    const panel = await screen.findByRole("region", { name: "Local printers" });
    fireEvent.change(within(panel).getByRole("textbox", { name: "Search printers" }), {
      target: { value: "plotter" },
    });

    expect(await within(panel).findByText("No printers match “plotter”.")).toBeTruthy();
  });

  it("shares the queues the user selects", async () => {
    vi.mocked(ipc.setSharedPrinters).mockResolvedValue(printersWith("Zebra"));
    const { container } = render(<App />);

    await sharingPanel(container);
    fireEvent.click(within(printerRow(container, "Zebra")).getByRole("checkbox"));

    expect(ipc.setSharedPrinters).toHaveBeenCalledWith(["Zebra"]);
    await waitFor(() => expect(printerRow(container, "Zebra").dataset.shared).toBe("true"));
    expect(printerRow(container, "HP LaserJet").dataset.shared).toBe("false");
  });

  it("shows the certificate fingerprint a client has to approve", async () => {
    render(<App />);
    navigateTo("Share printers");

    const fingerprint = await screen.findByTestId("server-fingerprint");
    expect(fingerprint.textContent).toBe(IDENTITY.fingerprint);
    expect(screen.getByText("Port 48631")).toBeTruthy();
    expect(screen.getByText(/Clients verify this fingerprint/)).toBeTruthy();
  });

  it("copies the server fingerprint only after the user asks", async () => {
    const originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    try {
      render(<App />);
      navigateTo("Share printers");
      await screen.findByTestId("server-fingerprint");
      fireEvent.click(screen.getByRole("button", { name: "Copy" }));

      expect(await screen.findByText("Fingerprint copied.")).toBeTruthy();
      expect(writeText).toHaveBeenCalledWith(IDENTITY.fingerprint);
    } finally {
      if (originalClipboard) {
        Object.defineProperty(navigator, "clipboard", originalClipboard);
      } else {
        Reflect.deleteProperty(navigator, "clipboard");
      }
    }
  });

  it("starts Server Sharing without a separate firewall action", async () => {
    vi.mocked(ipc.startService).mockResolvedValue(runtimeWith("running", "running"));
    const { container } = render(<App />);
    await sharingPanel(container);

    expect(screen.queryByRole("button", { name: "Allow client connections" })).toBeNull();
    navigateTo("Settings");
    const server = serviceCard(container, "server-sharing");
    fireEvent.click(within(server).getByRole("button", { name: "Start" }));

    await waitFor(() => expect(server.dataset.state).toBe("running"));
    expect(ipc.startService).toHaveBeenCalledWith("server-sharing");
  });

  it("starts and stops sharing from user intent in Share printers workspace", async () => {
    vi.mocked(networkChannel.getNetworkChannelStatus).mockResolvedValue(true);
    vi.mocked(ipc.listLocalPrinters).mockResolvedValue(printersWith("HP LaserJet"));
    vi.mocked(ipc.startService).mockResolvedValue(runtimeWith("running", "running"));
    vi.mocked(ipc.stopService).mockResolvedValue(runtimeWith("running", "stopped"));

    render(<App />);
    navigateTo("Share printers");

    // Local queues already has a shared printer (HP LaserJet)
    const startBtn = await screen.findByRole("button", { name: "Start sharing" });
    expect(startBtn).toBeTruthy();
    fireEvent.click(startBtn);

    await waitFor(() => {
      expect(ipc.startService).toHaveBeenCalledWith("server-sharing");
    });

    // When running, shows Stop sharing
    const stopBtn = await screen.findByRole("button", { name: "Stop sharing" });
    expect(stopBtn).toBeTruthy();
    fireEvent.click(stopBtn);

    await waitFor(() => {
      expect(ipc.stopService).toHaveBeenCalledWith("server-sharing");
    });
  });

  it("prompts for Network Channel when starting sharing without a configured channel", async () => {
    vi.mocked(networkChannel.getNetworkChannelStatus).mockResolvedValue(false);
    vi.mocked(ipc.listLocalPrinters).mockResolvedValue(printersWith("HP LaserJet"));
    vi.mocked(networkChannel.configureNetworkChannel).mockResolvedValue(true);
    vi.mocked(ipc.startService).mockResolvedValue(runtimeWith("running", "running"));

    render(<App />);
    navigateTo("Share printers");

    const startBtn = await screen.findByRole("button", { name: "Start sharing" });
    fireEvent.click(startBtn);

    // Shows contextual prompt
    expect(
      await screen.findByRole("region", { name: "Set Network Channel before sharing" }),
    ).toBeTruthy();

    // Cancel prompt leaves sharing off and preserves selection
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(
      screen.queryByRole("region", { name: "Set Network Channel before sharing" }),
    ).toBeNull();
    expect(ipc.startService).not.toHaveBeenCalled();

    // Entering secret saves and starts sharing
    fireEvent.click(screen.getByRole("button", { name: "Start sharing" }));
    fireEvent.change(screen.getByLabelText("Network Channel secret"), {
      target: { value: "secret123" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save & Start sharing" }));

    await waitFor(() => {
      expect(networkChannel.configureNetworkChannel).toHaveBeenCalledWith("secret123");
      expect(ipc.startService).toHaveBeenCalledWith("server-sharing");
    });
  });

  it("shows the stable error code when a change is rejected and re-reads the queues", async () => {
    vi.mocked(ipc.setSharedPrinters).mockRejectedValue({
      code: "invalid-input",
      message: "'Ghost' is not a local printer queue",
    });
    const { container } = render(<App />);

    await sharingPanel(container);
    fireEvent.click(within(printerRow(container, "Zebra")).getByRole("checkbox"));

    const panel = screen.getByRole("region", { name: "Local printers" });
    const alert = await within(panel).findByRole("alert");
    expect(alert.textContent).toContain("invalid-input");
    expect(alert.textContent).toContain("'Ghost' is not a local printer queue");
    // A rejected change re-reads the queues, so the checkbox cannot show a state the server is not
    // in.
    await waitFor(() => expect(ipc.listLocalPrinters).toHaveBeenCalledTimes(2));
    expect(printerRow(container, "Zebra").dataset.shared).toBe("false");
  });

  it("explains when this platform cannot list printer queues", async () => {
    vi.mocked(ipc.listLocalPrinters).mockRejectedValue({
      code: "unsupported",
      message: "listing local printer queues is only supported on Windows",
    });
    render(<App />);
    navigateTo("Share printers");

    const panel = await screen.findByRole("region", { name: "Local printers" });
    const alert = await within(panel).findByRole("alert");
    expect(alert.textContent).toContain("unsupported");
    expect(alert.textContent).toContain("only supported on Windows");
  });
});
describe("Network Channel panel", () => {
  it("gives the visibility button a field-specific name and keyboard focus", async () => {
    render(<App />);
    navigateTo("Settings");

    const panel = await screen.findByRole("region", { name: "Print authorization" });
    const input = within(panel).getByLabelText("Network Channel") as HTMLInputElement;
    const showButton = within(panel).getByRole("button", { name: "Show Network Channel" });

    expect(input.type).toBe("password");
    expect(showButton.tabIndex).toBe(0);
    fireEvent.click(showButton);
    expect(input.type).toBe("text");
    expect(within(panel).getByRole("button", { name: "Hide Network Channel" })).toBeTruthy();
  });

  it("saves a server channel without displaying the secret again", async () => {
    vi.mocked(networkChannel.configureNetworkChannel).mockResolvedValue(true);
    render(<App />);
    navigateTo("Settings");

    const input = screen.getByLabelText("Network Channel") as HTMLInputElement;
    const value = `network-${Date.now()}-${Math.random()}`;
    fireEvent.change(input, { target: { value } });
    fireEvent.click(screen.getByRole("button", { name: "Save Network Channel" }));

    expect(await screen.findByText(/Network Channel updated\./)).toBeTruthy();
    expect(networkChannel.configureNetworkChannel).toHaveBeenCalledWith(value);
    expect(input.value).toBe("");
    expect(screen.queryByText(value)).toBeNull();
  });
});

describe("nearby servers panel", () => {
  it("lists a server that advertises itself and reviews it without typing an address", async () => {
    vi.mocked(discovery.listNearbyServers).mockResolvedValue(nearbyWith("DESKTOP-ABC"));
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue({
      address: "192.0.2.10:8631",
      current_fingerprint:
        "11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF",
      previous_fingerprint: null,
      trusted: false,
    });
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    const dialog = await screen.findByRole("dialog", { name: "Find a server" });
    expect(await within(dialog).findByText("DESKTOP-ABC")).toBeTruthy();
    expect(within(dialog).getByText("192.0.2.10:8631")).toBeTruthy();
    expect(within(dialog).getByText(/Advertised \(unverified\): Zebra/)).toBeTruthy();

    const serverOption = within(dialog).getByText("DESKTOP-ABC").closest(".wizard-server-option") as HTMLElement;
    fireEvent.click(within(serverOption).getByRole("button", { name: "Review identity" }));

    // Reviewing a discovered server opens the guided setup dialog and inspects its certificate
    await waitFor(() =>
      expect(serverConnections.inspectServerConnection).toHaveBeenCalledWith("192.0.2.10:8631"),
    );
    expect(await screen.findByRole("dialog", { name: "Check the server identity" })).toBeTruthy();
    expect(serverConnections.listServerConnectionPrinters).not.toHaveBeenCalled();
  });

  it("follows the servers the shell publishes", async () => {
    let publish: ((servers: NearbyServers) => void) | undefined;
    vi.mocked(discovery.onNearbyServers).mockImplementation(async (handler) => {
      publish = handler;
      return () => {};
    });
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    const dialog = await screen.findByRole("dialog", { name: "Find a server" });
    await waitFor(() => expect(publish).toBeDefined());
    publish?.(nearbyWith("DESKTOP-ZULU"));

    expect(await within(dialog).findByText("DESKTOP-ZULU")).toBeTruthy();
  });

  it("keeps working by address when discovery finds nothing", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    const dialog = await screen.findByRole("dialog", { name: "Find a server" });
    expect(
      await within(dialog).findByText(/No servers found on this network/i),
    ).toBeTruthy();
    // Clicking Add printer allows manual address entry
    expect(within(dialog).getByLabelText("Or enter a server address")).toBeTruthy();
  });

  it("exposes the advertised version and displays a non-blocking version drift badge when server is newer", async () => {
    vi.mocked(discovery.listNearbyServers).mockResolvedValue({
      servers: [
        {
          name: "DESKTOP-MATCH",
          address: "192.0.2.10:8631",
          printers: ["Zebra"],
          version: CLIENT_VERSION,
        },
        {
          name: "DESKTOP-NEWER",
          address: "192.0.2.11:8631",
          printers: ["Canon"],
          version: "4.0.0",
        },
        {
          name: "DESKTOP-LEGACY",
          address: "192.0.2.12:8631",
          printers: ["HP"],
          version: null,
        },
      ],
    });
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue({
      address: "192.0.2.11:8631",
      current_fingerprint:
        "11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF",
      previous_fingerprint: null,
      trusted: false,
    });

    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));

    const dialog = await screen.findByRole("dialog", { name: "Find a server" });

    // Matching version server displays version badge without drift badge
    expect(await within(dialog).findByText("DESKTOP-MATCH")).toBeTruthy();
    expect(within(dialog).getByText(`v${CLIENT_VERSION}`)).toBeTruthy();

    // Legacy server displays without version badge or drift badge
    expect(await within(dialog).findByText("DESKTOP-LEGACY")).toBeTruthy();

    // Newer server displays version badge AND version-drift advisory badge
    expect(await within(dialog).findByText("DESKTOP-NEWER")).toBeTruthy();
    expect(within(dialog).getByText("v4.0.0")).toBeTruthy();
    const driftBadge = within(dialog).getByText(/Newer server release/);
    expect(driftBadge).toBeTruthy();

    // Version drift does not block review identity action
    const newerServerItem = within(dialog).getByText("DESKTOP-NEWER").closest(".wizard-server-option") as HTMLElement;
    const reviewButton = within(newerServerItem).getByRole("button", { name: "Review identity" });
    expect((reviewButton as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(reviewButton);

    await waitFor(() =>
      expect(serverConnections.inspectServerConnection).toHaveBeenCalledWith("192.0.2.11:8631"),
    );
  });
});

describe("startup panel", () => {
  it("reports that ShaPrint starts with the user's login and how to quit it", async () => {
    render(<App />);
    navigateTo("Settings");

    const panel = await screen.findByRole("region", { name: "Startup" });
    const toggle = (await within(panel).findByLabelText(
      /Start ShaPrint when I sign in to Windows/,
    )) as HTMLInputElement;
    expect(toggle.checked).toBe(true);
    expect(panel.textContent).toContain("shaprint-desktop.exe");
    expect(panel.textContent).toContain("Quit ShaPrint");
  });

  it("turns login startup off without asking for administrator permission", async () => {
    vi.mocked(startupApi.setStartupEnabled).mockResolvedValue({
      supported: true,
      enabled: false,
      command: '"C:\\Program Files\\ShaPrint\\shaprint-desktop.exe" --background',
    });
    render(<App />);
    navigateTo("Settings");

    const panel = await screen.findByRole("region", { name: "Startup" });
    const toggle = (await within(panel).findByLabelText(
      /Start ShaPrint when I sign in to Windows/,
    )) as HTMLInputElement;
    fireEvent.click(toggle);

    await waitFor(() => expect(startupApi.setStartupEnabled).toHaveBeenCalledWith(false));
    await waitFor(() => expect(toggle.checked).toBe(false));
    // Enabling or disabling login startup never elevates.
  });

  it("says when this platform cannot register login startup", async () => {
    vi.mocked(startupApi.getStartupStatus).mockResolvedValue({
      supported: false,
      enabled: false,
      command: "",
    });
    render(<App />);
    navigateTo("Settings");

    const panel = await screen.findByRole("region", { name: "Startup" });
    expect(
      await within(panel).findByText(/Starting ShaPrint at login is available on Windows/i),
    ).toBeTruthy();
    expect(
      within(panel).queryByLabelText(/Start ShaPrint when I sign in to Windows/),
    ).toBeNull();
  });
});

describe("previous app panel", () => {
  it("reports the Network Channel it took and asks for the printers again", async () => {
    vi.mocked(legacyApi.getLegacyImportReport).mockResolvedValue(LEGACY_REPORT);
    render(<App />);
    navigateTo("Settings");

    const panel = await screen.findByRole("region", { name: "Previous ShaPrint app" });
    expect(await within(panel).findByText(LEGACY_REPORT.channel_note)).toBeTruthy();
    expect(within(panel).getByText(/Network Channel: Imported/)).toBeTruthy();
    // The previous app's queues are never activated: the user selects them again here.
    expect(within(panel).getByTestId("reselect-queues").textContent).toContain(
      "Select your printers again",
    );
    expect(within(panel).getByText("Automatic updates")).toBeTruthy();
    expect(within(panel).getByText(/does not update itself yet/)).toBeTruthy();
  });

  it("follows the import the shell finishes", async () => {
    let publish: ((report: LegacyImportReport) => void) | undefined;
    vi.mocked(legacyApi.onLegacyImport).mockImplementation(async (handler) => {
      publish = handler;
      return () => {};
    });
    render(<App />);
    navigateTo("Settings");

    const panel = await screen.findByRole("region", { name: "Previous ShaPrint app" });
    await waitFor(() => expect(publish).toBeDefined());
    publish?.(LEGACY_REPORT);

    expect(await within(panel).findByText(LEGACY_REPORT.channel_note)).toBeTruthy();
  });

  it("says nothing was found when there was no previous app", async () => {
    render(<App />);
    navigateTo("Settings");

    const panel = await screen.findByRole("region", { name: "Previous ShaPrint app" });
    expect(
      await within(panel).findByText(/No settings from a previous ShaPrint app were found/i),
    ).toBeTruthy();
    expect(within(panel).queryByTestId("reselect-queues")).toBeNull();
    expect(within(panel).queryByRole("button", { name: "Import again" })).toBeNull();
  });

  it("checks again without asking for administrator permission", async () => {
    vi.mocked(legacyApi.getLegacyImportReport).mockResolvedValue({
      ...LEGACY_REPORT,
      channel: "importable",
      channel_note: "The previous ShaPrint app's Network Channel is ready to be imported.",
    });
    vi.mocked(legacyApi.importLegacySettings).mockResolvedValue(LEGACY_REPORT);
    render(<App />);
    navigateTo("Settings");

    const panel = await screen.findByRole("region", { name: "Previous ShaPrint app" });
    fireEvent.click(await within(panel).findByRole("button", { name: "Import again" }));

    await waitFor(() => expect(legacyApi.importLegacySettings).toHaveBeenCalledTimes(1));
    expect(await within(panel).findByText(LEGACY_REPORT.channel_note)).toBeTruthy();
  });
});
