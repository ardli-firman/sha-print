import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import * as ipc from "./api/ipc";
import type { LocalPrinters, RuntimeStatus, ServerIdentity } from "./api/types";

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

const runtimeWith = (
  proxy: RuntimeStatus["services"][number]["state"],
  sharing: RuntimeStatus["services"][number]["state"],
): RuntimeStatus => ({
  services: [
    { id: "client-proxy", state: proxy, detail: proxy === "running" ? "running" : "" },
    { id: "server-sharing", state: sharing, detail: sharing === "running" ? "running" : "" },
  ],
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
