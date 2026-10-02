/**
 * Typed wrappers around the shell's print-failure command and event.
 *
 * A failure is the shell's explanation of why a print job did not come out. It carries a stable
 * code, a message, and the action that resolves it — never the Network Channel or any part of the
 * document. `null` means nothing is outstanding.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { PrintFailure } from "./types";

/** Event emitted whenever the shell reports or clears a print failure. */
export const PRINT_FAILURE_EVENT = "runtime://print-failure";

/** The latest print failure, or `null` when nothing has failed since the last dismissal. */
export function getPrintFailures(): Promise<PrintFailure | null> {
  return invoke<PrintFailure | null>("get_print_failures");
}

/** Dismisses the latest print failure; resolves with the resulting state. */
export function dismissPrintFailure(): Promise<PrintFailure | null> {
  return invoke<PrintFailure | null>("dismiss_print_failure");
}

/** Subscribes to print failures; resolve the returned function to unsubscribe. */
export function onPrintFailure(
  handler: (failure: PrintFailure | null) => void,
): Promise<UnlistenFn> {
  return listen<PrintFailure | null>(PRINT_FAILURE_EVENT, (event) => handler(event.payload));
}
