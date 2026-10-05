import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const UPDATE_STATUS_EVENT = "updater://status";

export type UpdateState =
  | { state: "idle" | "checking" }
  | { state: "available" | "downloading"; version: string; notes?: string | null }
  | { state: "ready_to_restart"; version: string; restart_requested: boolean }
  | { state: "failed"; message: string };

export interface UpdateStatus {
  current_version: string;
  update: UpdateState;
  waiting_for_jobs: boolean;
}

export function getUpdateStatus(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("get_update_status");
}

export function checkForUpdates(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("check_for_updates");
}

export function applyUpdateAndRestart(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("apply_update_and_restart");
}

export function onUpdateStatus(handler: (status: UpdateStatus) => void): Promise<UnlistenFn> {
  return listen<UpdateStatus>(UPDATE_STATUS_EVENT, (event) => handler(event.payload));
}
