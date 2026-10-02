/**
 * Typed wrappers around the shell's login-startup commands.
 *
 * Registration is per user, so turning this on never needs administrator permission. The command
 * the shell reports is the plain program path: it carries no credential.
 */

import { invoke } from "@tauri-apps/api/core";

/** Whether ShaPrint starts with this user's Windows login. */
export interface StartupStatus {
  /** Whether this platform registers login startup at all. */
  supported: boolean;
  /** Whether the registration matches this installation. */
  enabled: boolean;
  /** The command a login launch runs; empty when the program could not be located. */
  command: string;
}

export function getStartupStatus(): Promise<StartupStatus> {
  return invoke<StartupStatus>("get_startup_status");
}

/** Turns starting with Windows on or off; resolves with the resulting state. */
export function setStartupEnabled(enabled: boolean): Promise<StartupStatus> {
  return invoke<StartupStatus>("set_startup_enabled", { enabled });
}
