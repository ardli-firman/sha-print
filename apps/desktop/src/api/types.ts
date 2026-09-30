/**
 * IPC payload types. These mirror the Rust DTOs in `src-tauri/src/ipc/dto.rs`; ids, states, and
 * error codes are stable strings and must not be renamed on one side only.
 */

/** Runtime services the desktop shell supervises. */
export type ServiceId = "client-proxy" | "server-sharing";

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
  "internal",
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

/** Failure returned by the shell: a stable code plus a human-readable message. */
export interface AppError {
  code: ErrorCode;
  message: string;
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

/** What a setup action did. */
export interface SetupOutcome {
  /** Whether administrator permission was requested for the action. */
  elevated: boolean;
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
