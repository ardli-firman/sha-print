import { invoke } from "@tauri-apps/api/core";
import type { DiagnosticCheck, RecognisedClientQueue } from "./types";

export interface LocalTroubleshootingSnapshot {
  target: RecognisedClientQueue;
  version: string;
  checks: DiagnosticCheck[];
  queue_empty: boolean;
  test_print_enabled: boolean;
  repair_enabled: boolean;
  attempted_actions: { action: string; outcome: string }[];
}

export function inspectLocalClientQueue(target: RecognisedClientQueue): Promise<LocalTroubleshootingSnapshot> {
  return invoke("inspect_local_client_queue", { target });
}
