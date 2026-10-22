/**
 * IPC payload types. These mirror the Rust DTOs in `src-tauri/src/ipc/dto.rs`; ids, states, and
 * error codes are stable strings and must not be renamed on one side only.
 */

/** Runtime services the desktop shell supervises. */
export type ServiceId = "client-proxy" | "server-sharing" | "server-discovery";

/** Lifecycle state of a supervised service. */
export type ServiceState = "stopped" | "starting" | "running" | "stopping" | "failed";

/** Status of one supervised service. */
export interface ServiceStatus {
  id: ServiceId;
  state: ServiceState;
  /** Short note about the current state; never contains credentials or print job content. */
  detail: string;
}

/** Status of every supervised service. */
export interface RuntimeStatus {
  services: ServiceStatus[];
  network_channel_configured?: boolean | null;
  readiness?: string | null;
  shared_printers?: string[] | null;
  client_queues?: string[] | null;
  failure_code?: ErrorCode | string | null;
  recent_failure?: PrintFailure | null;
}

/** One diagnostic check result. */
export interface DiagnosticCheck {
  /** Distinct subsystem checked: `daemon`, `ports`, `cups`, `trust`, `credential`, `firewall`. */
  check: string;
  /** Stable check status: `ok`, `failed`, or `warning`. */
  status: "ok" | "failed" | "warning" | string;
  /** Stable error code when failed, for example `server-unavailable`. */
  code?: ErrorCode | string | null;
  /** Safe human-readable summary of the condition. Never contains credentials or print content. */
  detail: string;
  /** Concrete recovery action for resolving the failure. */
  recovery?: string | null;
}

/** Comprehensive diagnostic view of the ShaPrint environment. */
export interface DiagnosticsReport {
  checks: DiagnosticCheck[];
  healthy: boolean;
  recent_failure?: PrintFailure | null;
}

/**
 * Which control plane the desktop shell administers: `local` for the in-process runtime this
 * window owns, `machine` for the system daemon it talks to over the local control socket.
 */
export type ControlMode = "local" | "machine";

/** Stable error codes the shell can report. */
export const ERROR_CODES = [
  "invalid-input",
  "unknown-service",
  "invalid-state",
  "timeout",
  "unsupported",
  "server-unavailable",
  "server-untrusted",
  "server-identity-changed",
  "not-authorized",
  "printer-not-shared",
  "queue-unavailable",
  "internal",
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

/** Failure returned by the shell: a stable code plus a human-readable message. */
export interface AppError {
  code: ErrorCode;
  message: string;
}

/** Which print path reported a failure. */
export type PrintFailurePath = "client-forwarding" | "server-submission";

/** A print attempt that failed, with the one action that resolves it. */
export interface PrintFailure {
  /** The path that failed: forwarding from this computer, or submitting to a server queue. */
  path: PrintFailurePath;
  /** Stable code for the condition, for example `server-unavailable`. */
  code: ErrorCode;
  /** The target printer queue if known. */
  printer?: string | null;
  /** What happened; never contains the Network Channel or document contents. */
  message: string;
  /** The next action for the user. */
  recovery: string;
  /** When the failure was observed, as milliseconds since the Unix epoch. */
  observed_at_ms: number;
}

/** One local printer queue and whether the server shares it. */
export interface LocalPrinter {
  name: string;
  shared: boolean;
}

/** Every local printer queue with its sharing state. */
export interface LocalPrinters {
  printers: LocalPrinter[];
}

/** The server identity a client user approves, and where clients connect. */
export interface ServerIdentity {
  /** Uppercase, colon-separated SHA-256 fingerprint of the server certificate. */
  fingerprint: string;
  /** Port the sharing endpoint listens on. */
  port: number;
}

/** A server the client can see on the local network, before any trust decision. */
export interface NearbyServer {
  /** Label the server advertises for itself. */
  name: string;
  /**
   * Address to review and approve before any printer is listed. It is also what tells two servers
   * apart when they advertise the same label.
   */
  address: string;
  /** The printers the server says it shares. */
  printers: string[];
  /** Semantic version advertised by the server, if any. */
  version?: string | null;
}

/** Every server currently visible on the local network. */
export interface NearbyServers {
  servers: NearbyServer[];
}

/** A native Windows queue the shell installed for a shared printer. */
export interface ClientQueue {
  /** Queue name as it appears in Windows print dialogs. */
  queue_name: string;
  /** Canonical host:port of the server that shares the printer. */
  server_address: string;
  /** Printer queue name on that server. */
  printer_name: string;
  /** IPP URI the installed queue routes through; it points at the local proxy. */
  uri: string;
}

/** A recognised ShaPrint client queue read from the current Windows spooler state. */
export interface RecognisedClientQueue {
  /** Exact queue name as it appears in the Windows spooler. */
  queue_name: string;
  /** Canonical host:port of the server that shares the printer. */
  server_address: string;
  /** Printer queue name on that server. */
  printer_name: string;
}

const KNOWN_ERROR_CODES: Record<string, true> = Object.fromEntries(
  ERROR_CODES.map((code) => [code, true]),
);

export function isErrorCode(value: unknown): value is ErrorCode {
  return typeof value === "string" && KNOWN_ERROR_CODES[value] === true;
}

/**
 * Normalises anything a rejected IPC call can carry into an [`AppError`]. Unknown shapes become
 * `internal` so the UI always has a code to show.
 */
export function toAppError(cause: unknown): AppError {
  if (typeof cause === "object" && cause !== null) {
    const candidate = cause as { code?: unknown; message?: unknown };
    if (isErrorCode(candidate.code)) {
      return {
        code: candidate.code,
        message: typeof candidate.message === "string" ? candidate.message : candidate.code,
      };
    }
  }
  if (typeof cause === "string") {
    return { code: "internal", message: cause };
  }
  if (cause instanceof Error) {
    return { code: "internal", message: cause.message };
  }
  return { code: "internal", message: "unexpected error" };
}
