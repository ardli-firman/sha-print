import { invoke } from "@tauri-apps/api/core";

import type { ClientQueue, RecognisedClientQueue } from "./types";

export interface ServerConnectionReview {
  address: string;
  current_fingerprint: string;
  previous_fingerprint: string | null;
  trusted: boolean;
}
export interface ServerConnectionPrinters { address: string; printers: string[] }
export interface TrustedServer { address: string; fingerprint: string }
export type TrustedServerStatus = "online" | "offline" | "identity_changed";
export interface TrustedServerProbe {
  address: string;
  status: TrustedServerStatus;
  approved_fingerprint: string;
  current_fingerprint: string | null;
  printers: string[];
}

export function listTrustedServers(): Promise<TrustedServer[]> {
  return invoke("list_trusted_servers");
}
export function probeTrustedServer(address: string): Promise<TrustedServerProbe> {
  return invoke("probe_trusted_server", { address });
}
export function forgetTrustedServer(address: string): Promise<void> {
  return invoke("forget_trusted_server", { address });
}

export function inspectServerConnection(address: string): Promise<ServerConnectionReview> {
  return invoke("inspect_server_connection", { address });
}
export function approveServerConnection(address: string, fingerprint: string): Promise<ServerConnectionReview> {
  return invoke("approve_server_connection", { address, fingerprint });
}
export function listServerConnectionPrinters(address: string): Promise<ServerConnectionPrinters> {
  return invoke("list_server_connection_printers", { address });
}
/** Installs the native Windows queue for a printer the trusted server shares. */
export function installPrinterQueue(serverAddress: string, printerName: string): Promise<ClientQueue> {
  return invoke("install_printer_queue", { serverAddress, printerName });
}

/** Lists recognised ShaPrint client queues from the current Windows spooler state. */
export function listRecognisedClientQueues(): Promise<RecognisedClientQueue[]> {
  return invoke("list_recognised_client_queues");
}

/** Opens Windows Printers & scanners settings through the desktop shell. */
export function openPrintersSettings(): Promise<void> {
  return invoke("open_printers_settings");
}
