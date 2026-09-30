/**
 * Typed wrappers around the shell's Tauri commands and status event.
 *
 * Commands return the full status snapshot, so callers can render the outcome of an action
 * without a follow-up fetch. Failures are normalised by `toAppError`.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { RuntimeStatus, ServiceId } from "./types";

/** Event emitted whenever the shell publishes a new status snapshot. */
export const RUNTIME_STATUS_EVENT = "runtime://status";

/** Action callable from the UI: start or stop one service. */
export type ServiceAction = (id: ServiceId) => Promise<RuntimeStatus>;

export function getRuntimeStatus(): Promise<RuntimeStatus> {
  return invoke<RuntimeStatus>("get_runtime_status");
}

export const startService: ServiceAction = (id) => invoke<RuntimeStatus>("start_service", { id });

export const stopService: ServiceAction = (id) => invoke<RuntimeStatus>("stop_service", { id });

/** Subscribes to live status changes; resolve the returned function to unsubscribe. */
export function onRuntimeStatus(handler: (status: RuntimeStatus) => void): Promise<UnlistenFn> {
  return listen<RuntimeStatus>(RUNTIME_STATUS_EVENT, (event) => handler(event.payload));
}

export type { UnlistenFn };
