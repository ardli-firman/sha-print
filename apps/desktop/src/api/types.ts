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
}

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
}

/** Every server currently visible on the local network. */
export interface NearbyServers {
  servers: NearbyServer[];
}

/** What a setup action did. */
export interface SetupOutcome {
  /** Whether administrator permission was requested for the action. */
  elevated: boolean;
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
