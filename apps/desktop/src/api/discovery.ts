/**
 * Typed wrappers around the shell's nearby-server command and event.
 *
 * Discovery is a hint: the address it reports still goes through `inspectServerConnection` and an
 * explicit fingerprint approval before any printer is listed.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { NearbyServers } from "./types";

/** Event emitted whenever the shell publishes a new list of nearby servers. */
export const NEARBY_SERVERS_EVENT = "discovery://servers";

/** Lists the servers currently visible on the local network. */
export function listNearbyServers(): Promise<NearbyServers> {
  return invoke<NearbyServers>("list_nearby_servers");
}

/** Subscribes to nearby-server changes; resolve the returned function to unsubscribe. */
export function onNearbyServers(handler: (servers: NearbyServers) => void): Promise<UnlistenFn> {
  return listen<NearbyServers>(NEARBY_SERVERS_EVENT, (event) => handler(event.payload));
}
