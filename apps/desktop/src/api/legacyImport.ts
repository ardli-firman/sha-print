/**
 * Typed wrappers around the shell's import from the previous .NET ShaPrint app.
 *
 * The report explains what moved and what did not. It never carries the Network Channel itself:
 * only whether it was imported.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Event emitted when the shell has finished an import. */
export const LEGACY_IMPORT_EVENT = "legacy://import";

/** What happened to the previous app's Network Channel. */
export type LegacyChannelOutcome =
  | "importable"
  | "imported"
  | "kept-existing"
  | "absent"
  | "unreadable";

/** One setting from the previous app that this app does not take. */
export interface LegacySetting {
  key: string;
  label: string;
  /** Why this app does not take it. */
  reason: string;
}

/** What the move from the previous app did, or would do. */
export interface LegacyImportReport {
  /** Whether the previous app left settings for this user. */
  found: boolean;
  /** Stable channel outcome. */
  channel: LegacyChannelOutcome;
  /** The sentence to show for that outcome; never contains the channel itself. */
  channel_note: string;
  settings: LegacySetting[];
  /** Whether the previous app had queues the user has to select again. */
  queues_need_reselection: boolean;
}

export function getLegacyImportReport(): Promise<LegacyImportReport> {
  return invoke<LegacyImportReport>("get_legacy_import_report");
}

/** Runs the import again; importing twice changes nothing. */
export function importLegacySettings(): Promise<LegacyImportReport> {
  return invoke<LegacyImportReport>("import_legacy_settings");
}

/** Subscribes to completed imports; resolve the returned function to unsubscribe. */
export function onLegacyImport(
  handler: (report: LegacyImportReport) => void,
): Promise<UnlistenFn> {
  return listen<LegacyImportReport>(LEGACY_IMPORT_EVENT, (event) => handler(event.payload));
}
