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
  allowSharingAccess: vi.fn(),
}));

vi.mock("./api/networkChannel", () => ({
  getNetworkChannelStatus: vi.fn(),
  configureNetworkChannel: vi.fn(),
}));

vi.mock("./api/serverConnections", () => ({
  inspectServerConnection: vi.fn(),
  approveServerConnection: vi.fn(),
  listServerConnectionPrinters: vi.fn(),
  installPrinterQueue: vi.fn(),
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
  port: 8631,
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

type WorkspacePage = "Share" | "Connect" | "Settings";

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

    fireEvent.click(screen.getByRole("button", { name: "Check for Updates" }));

    expect((await screen.findByRole("alert")).textContent).toMatch(/GitHub Releases could not be reached/);
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
});

describe("workspace navigation", () => {
  it("starts on Share and keeps Connect and Settings workflows in their own sections", async () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "Share printers", level: 1 })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Share" }).getAttribute("aria-current")).toBe("page");
    expect(screen.queryByRole("region", { name: "Nearby servers" })).toBeNull();

    navigateTo("Connect");
    expect(await screen.findByRole("heading", { name: "Connect to a server", level: 1 })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Nearby servers" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Local printers" })).toBeNull();

    navigateTo("Settings");
    expect(await screen.findByRole("heading", { name: "Settings", level: 1 })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Print authorization" })).toBeTruthy();
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
  it("shows the setup sequence and describes inspection as an identity review", async () => {
    const review = {
      address: "printer.example:8631",
      current_fingerprint: IDENTITY.fingerprint,
      previous_fingerprint: null,
      trusted: false,
    };
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue(review);
    render(<App />);
    navigateTo("Connect");
    fireEvent.click(screen.getByRole("button", { name: "Guided setup" }));

    expect(await screen.findByRole("dialog", { name: "Find a server" })).toBeTruthy();
    expect(screen.getByRole("list", { name: "Printer setup steps" })).toBeTruthy();
    expect(screen.getByText("Server").closest("li")?.getAttribute("aria-current")).toBe("step");

    fireEvent.change(screen.getByLabelText("Or enter a server address"), {
      target: { value: "printer.example" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Review identity" }));

    expect(await screen.findByRole("heading", { name: "Check the server identity" })).toBeTruthy();
    expect(screen.getByText("Identity").closest("li")?.getAttribute("aria-current")).toBe("step");
    expect(screen.getByRole("button", { name: "Approve fingerprint" })).toBeTruthy();
    expect(serverConnections.listServerConnectionPrinters).not.toHaveBeenCalled();
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

    const fingerprint = await screen.findByTestId("server-fingerprint");
    expect(fingerprint.textContent).toBe(IDENTITY.fingerprint);
    expect(screen.getByText("Port 8631")).toBeTruthy();
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

  it("asks for administrator permission only when the user opens client access", async () => {
    vi.mocked(ipc.allowSharingAccess).mockResolvedValue({ elevated: true });
    vi.mocked(ipc.setSharedPrinters).mockResolvedValue(printersWith("Zebra"));
    const { container } = render(<App />);

    await sharingPanel(container);
    fireEvent.click(within(printerRow(container, "Zebra")).getByRole("checkbox"));
    await waitFor(() => expect(printerRow(container, "Zebra").dataset.shared).toBe("true"));
    expect(ipc.allowSharingAccess).not.toHaveBeenCalled();

    fireEvent.click(await screen.findByRole("button", { name: "Allow client connections" }));

    await waitFor(() => expect(ipc.allowSharingAccess).toHaveBeenCalledTimes(1));
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

    const panel = await screen.findByRole("region", { name: "Local printers" });
    const alert = await within(panel).findByRole("alert");
    expect(alert.textContent).toContain("unsupported");
    expect(alert.textContent).toContain("only supported on Windows");
  });
});
describe("server connection panel", () => {
  it("does not list printers before explicit fingerprint approval", async () => {
    const review = {
      address: "printer.example:8631",
      current_fingerprint:
        "11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF",
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
    render(<App />);
    navigateTo("Connect");

    fireEvent.change(screen.getByLabelText("Server address"), {
      target: { value: "printer.example" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Inspect certificate" }));

    await screen.findByText(review.current_fingerprint);
    expect(serverConnections.listServerConnectionPrinters).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Approve this fingerprint" }));

    fireEvent.click(await screen.findByRole("button", { name: "Show shared printers" }));

    expect(await screen.findByText("Office Laser")).toBeTruthy();
    expect(serverConnections.approveServerConnection).toHaveBeenCalledWith(
      review.address,
      review.current_fingerprint,
    );
    expect(serverConnections.listServerConnectionPrinters).toHaveBeenCalledWith(
      review.address,
    );
  });

  it("installs a Windows queue for an approved server's shared printer", async () => {
    const review = {
      address: "printer.example:8631",
      current_fingerprint:
        "11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF",
      previous_fingerprint: null,
      trusted: true,
    };
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue(review);
    vi.mocked(serverConnections.listServerConnectionPrinters).mockResolvedValue({
      address: review.address,
      printers: ["Office Laser"],
    });
    vi.mocked(serverConnections.installPrinterQueue).mockResolvedValue({
      queue_name: "Office Laser (ShaPrint printer.example-8631)",
      server_address: review.address,
      printer_name: "Office Laser",
      uri: "ipp://127.0.0.1:8632/ipp/print/printer.example%3A8631/Office%20Laser",
    });
    render(<App />);
    navigateTo("Connect");

    fireEvent.change(screen.getByLabelText("Server address"), {
      target: { value: "printer.example" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Inspect certificate" }));
    fireEvent.click(await screen.findByRole("button", { name: "Show shared printers" }));
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Install a Windows queue for Office Laser",
      }),
    );

    // The installed queue is named after the printer and the server, so the user can find it.
    expect(await screen.findByText(/Office Laser \(ShaPrint printer\.example-8631\)/)).toBeTruthy();
    expect(serverConnections.installPrinterQueue).toHaveBeenCalledWith(
      review.address,
      "Office Laser",
    );
  });

  it("shows the action a user can take when the queue install fails", async () => {
    const review = {
      address: "printer.example:8631",
      current_fingerprint:
        "11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF",
      previous_fingerprint: null,
      trusted: true,
    };
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue(review);
    vi.mocked(serverConnections.listServerConnectionPrinters).mockResolvedValue({
      address: review.address,
      printers: ["Office Laser"],
    });
    vi.mocked(serverConnections.installPrinterQueue).mockRejectedValue({
      code: "invalid-state",
      message:
        'Could not install the Windows queue "Office Laser (ShaPrint printer.example-8631)" for printer "Office Laser": the Windows Print Spooler service is not running. Start it (services.msc), then try again.',
    });
    render(<App />);
    navigateTo("Connect");

    fireEvent.change(screen.getByLabelText("Server address"), {
      target: { value: "printer.example" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Inspect certificate" }));
    fireEvent.click(await screen.findByRole("button", { name: "Show shared printers" }));
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Install a Windows queue for Office Laser",
      }),
    );

    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toContain("Print Spooler service is not running");
    expect(banner.textContent).toContain("services.msc");
  });

  it("explains changed identity and requires explicit reapproval", async () => {
    vi.mocked(serverConnections.inspectServerConnection).mockResolvedValue({
      address: "printer.example:8631",
      current_fingerprint:
        "11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF",
      previous_fingerprint:
        "AA:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF",
      trusted: false,
    });
    render(<App />);
    navigateTo("Connect");

    fireEvent.change(screen.getByLabelText("Server address"), {
      target: { value: "printer.example" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Inspect certificate" }));

    expect(
      await screen.findByText(/identity changed\. Printers are blocked/i),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Explicitly reapprove this fingerprint" }),
    ).toBeTruthy();
    expect(serverConnections.listServerConnectionPrinters).not.toHaveBeenCalled();
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
    navigateTo("Connect");

    const panel = await screen.findByRole("region", { name: "Nearby servers" });
    expect(await within(panel).findByText("DESKTOP-ABC")).toBeTruthy();
    expect(within(panel).getByText("192.0.2.10:8631")).toBeTruthy();
    expect(within(panel).getByText(/Shares: Zebra/)).toBeTruthy();

    fireEvent.click(within(panel).getByRole("button", { name: "Review identity" }));

    // Reviewing a discovered server goes through the same inspection as a typed address, and the
    // address field shows what is being reviewed.
    await waitFor(() =>
      expect(serverConnections.inspectServerConnection).toHaveBeenCalledWith("192.0.2.10:8631"),
    );
    expect((screen.getByLabelText("Server address") as HTMLInputElement).value).toBe(
      "192.0.2.10:8631",
    );
    expect(serverConnections.listServerConnectionPrinters).not.toHaveBeenCalled();
  });

  it("follows the servers the shell publishes", async () => {
    let publish: ((servers: NearbyServers) => void) | undefined;
    vi.mocked(discovery.onNearbyServers).mockImplementation(async (handler) => {
      publish = handler;
      return () => {};
    });
    render(<App />);
    navigateTo("Connect");

    const panel = await screen.findByRole("region", { name: "Nearby servers" });
    await waitFor(() => expect(publish).toBeDefined());
    publish?.(nearbyWith("DESKTOP-ZULU"));

    expect(await within(panel).findByText("DESKTOP-ZULU")).toBeTruthy();
  });

  it("keeps working by address when discovery finds nothing", async () => {
    render(<App />);
    navigateTo("Connect");

    const panel = await screen.findByRole("region", { name: "Nearby servers" });
    expect(
      await within(panel).findByText(/Nothing found on this network/i),
    ).toBeTruthy();
    // The manual path is untouched by an empty discovery list.
    expect(screen.getByLabelText("Server address")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Inspect certificate" })).toBeTruthy();
  });

  it("exposes the advertised version and displays a non-blocking version drift badge when server is newer", async () => {
    vi.mocked(discovery.listNearbyServers).mockResolvedValue({
      servers: [
        {
          name: "DESKTOP-MATCH",
          address: "192.0.2.10:8631",
          printers: ["Zebra"],
          version: "3.1.1",
        },
        {
          name: "DESKTOP-NEWER",
          address: "192.0.2.11:8631",
          printers: ["Canon"],
          version: "3.2.0",
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
    navigateTo("Connect");

    const panel = await screen.findByRole("region", { name: "Nearby servers" });

    // Matching version server displays version badge without drift badge
    expect(await within(panel).findByText("DESKTOP-MATCH")).toBeTruthy();
    expect(within(panel).getByText("v3.1.1")).toBeTruthy();

    // Legacy server displays without version badge or drift badge
    expect(await within(panel).findByText("DESKTOP-LEGACY")).toBeTruthy();

    // Newer server displays version badge AND version-drift advisory badge
    expect(await within(panel).findByText("DESKTOP-NEWER")).toBeTruthy();
    expect(within(panel).getByText("v3.2.0")).toBeTruthy();
    const driftBadge = within(panel).getByText(/Newer server release/);
    expect(driftBadge).toBeTruthy();

    // Version drift does not block review identity action
    const newerServerItem = within(panel).getByText("DESKTOP-NEWER").closest("li")!;
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
    expect(ipc.allowSharingAccess).not.toHaveBeenCalled();
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
    expect(ipc.allowSharingAccess).not.toHaveBeenCalled();
  });
});
