import { invoke } from "@tauri-apps/api/core";

/** Returns only whether the server has a Network Channel configured. */
export function getNetworkChannelStatus(): Promise<boolean> {
  return invoke<boolean>("get_network_channel_status");
}

/** Sets or replaces the Network Channel; the secret is never returned. */
export function configureNetworkChannel(networkChannel: string): Promise<boolean> {
  return invoke<boolean>("configure_network_channel", { networkChannel });
}
