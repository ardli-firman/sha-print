import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import * as discovery from "./api/discovery";
import * as ipc from "./api/ipc";
import * as networkChannel from "./api/networkChannel";
import * as serverConnections from "./api/serverConnections";
import type { LocalPrinters, NearbyServers, RuntimeStatus, ServerIdentity } from "./api/types";

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
}));

vi.mock("./api/discovery", () => ({
  NEARBY_SERVERS_EVENT: "discovery://servers",
  listNearbyServers: vi.fn(),
  onNearbyServers: vi.fn(),
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
  servers: names.map((name) => ({
    instance: `${name}._shaprint-ipps._tcp.local.`,
    name,
    address: "192.0.2.10:8631",
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
  return screen.getByRole("region", { name: "Shared printers" });
}

// Testing Library only cleans up automatically when Vitest globals are turned on; this suite
// queries the whole document, so earlier renders must not stay behind.
afterEach(cleanup);

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(ipc.onRuntimeStatus).mockResolvedValue(() => {});
  vi.mocked(ipc.getRuntimeStatus).mockResolvedValue(runtimeWith("running", "stopped"));
  vi.mocked(ipc.listLocalPrinters).mockResolvedValue(printersWith());
  vi.mocked(ipc.getServerIdentity).mockResolvedValue(IDENTITY);
  vi.mocked(networkChannel.getNetworkChannelStatus).mockResolvedValue(false);
  vi.mocked(discovery.listNearbyServers).mockResolvedValue({ servers: [] });
  vi.mocked(discovery.onNearbyServers).mockResolvedValue(() => {});
});

describe("runtime status panel", () => {
  it("shows the live state of the proxy and sharing services", async () => {
    const { container } = render(<App />);

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

    await waitFor(() => expect(container.querySelector('[data-service="client-proxy"]')).not.toBeNull());
    const proxy = serviceCard(container, "client-proxy");
    fireEvent.click(within(proxy).getByRole("button", { name: "Start" }));

    const panel = screen.getByRole("region", { name: "Services" });
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

    const hint = await screen.findByText(/runtime is not reachable/i);
    expect(hint).toBeTruthy();
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
    expect(screen.getByText(/2 of 3 local queues are shared/)).toBeTruthy();
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
    expect(screen.getByText(/Clients connect to port 8631/)).toBeTruthy();
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

    const panel = screen.getByRole("region", { name: "Shared printers" });
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

    const panel = await screen.findByRole("region", { name: "Shared printers" });
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
  it("saves a server channel without displaying the secret again", async () => {
    vi.mocked(networkChannel.configureNetworkChannel).mockResolvedValue(true);
    render(<App />);

    const input = screen.getByLabelText("Network Channel") as HTMLInputElement;
    const value = `network-${Date.now()}-${Math.random()}`;
    fireEvent.change(input, { target: { value } });
    fireEvent.click(screen.getByRole("button", { name: "Set Network Channel" }));

    expect(await screen.findByText("Network Channel updated.")).toBeTruthy();
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

    const panel = await screen.findByRole("region", { name: "Nearby servers" });
    expect(await within(panel).findByText("DESKTOP-ABC")).toBeTruthy();
    expect(within(panel).getByText("192.0.2.10:8631")).toBeTruthy();
    expect(within(panel).getByText(/Shares: Zebra/)).toBeTruthy();

    fireEvent.click(within(panel).getByRole("button", { name: "Review certificate" }));

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

    const panel = await screen.findByRole("region", { name: "Nearby servers" });
    await waitFor(() => expect(publish).toBeDefined());
    publish?.(nearbyWith("DESKTOP-ZULU"));

    expect(await within(panel).findByText("DESKTOP-ZULU")).toBeTruthy();
  });

  it("keeps working by address when discovery finds nothing", async () => {
    render(<App />);

    const panel = await screen.findByRole("region", { name: "Nearby servers" });
    expect(
      await within(panel).findByText(/No ShaPrint server has advertised itself/i),
    ).toBeTruthy();
    // The manual path is untouched by an empty discovery list.
    expect(screen.getByLabelText("Server address")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Inspect certificate" })).toBeTruthy();
  });
});
