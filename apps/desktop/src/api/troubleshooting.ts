import { invoke } from "@tauri-apps/api/core";
import type { DiagnosticCheck, RecognisedClientQueue } from "./types";

/** Classified incorrect-output symptoms; the backend rejects any other value. */
export type UserSymptom = "clipping" | "shifted" | "incorrect-size" | "blank";

export interface LocalTroubleshootingSnapshot {
  target: RecognisedClientQueue;
  version: string;
  checks: DiagnosticCheck[];
  queue_empty: boolean;
  test_print_enabled: boolean;
  repair_enabled: boolean;
  session_id?: number;
  requested_media?: string;
  submission?: "not-sent" | "checking" | "accepted" | "rejected" | "uncertain" | "blocked";
  physical_result?: "unconfirmed" | "correct" | "no-output" | "incorrect";
  user_symptom?: UserSymptom;
  attempted_actions: { action: string; outcome: string }[];
}

export function inspectLocalClientQueue(target: RecognisedClientQueue): Promise<LocalTroubleshootingSnapshot> {
  return invoke("inspect_local_client_queue", { target });
}

export function refreshTestPrintSession(sessionId: number): Promise<LocalTroubleshootingSnapshot> {
  return invoke("refresh_test_print_session", { sessionId });
}
export function sendTestPrint(sessionId: number): Promise<LocalTroubleshootingSnapshot> {
  return invoke("send_test_print", { sessionId });
}
export function confirmTestPrint(
  sessionId: number,
  result: "correct" | "no-output" | "incorrect",
  symptom?: UserSymptom,
): Promise<LocalTroubleshootingSnapshot> {
  return invoke(
    "confirm_test_print",
    symptom ? { sessionId, result, symptom } : { sessionId, result },
  );
}
export function closeTestPrintSession(sessionId: number): Promise<void> {
  return invoke("close_test_print_session", { sessionId });
}
