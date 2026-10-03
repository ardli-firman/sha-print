import { invoke } from "@tauri-apps/api/core";

import type { ClientQueue } from "./types";

export interface ServerConnectionReview {
  address: string;
  current_fingerprint: string;
  previous_fingerprint: string | null;
  trusted: boolean;
}
export interface ServerConnectionPrinters { address: string; printers: string[] }

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
