import { useCallback, useEffect, useState } from "react";

import { listNearbyServers, onNearbyServers } from "@/api/discovery";
import type { UnlistenFn } from "@/api/ipc";
import { toAppError, type AppError, type NearbyServer } from "@/api/types";

/** What the nearby-servers panel needs to render. */
export interface NearbyServersController {
  /** Servers the shell currently sees, ordered by label. */
  servers: NearbyServer[];
  /** Most recent failure worth showing the user. */
  error: AppError | null;
  dismissError: () => void;
}

/**
 * Keeps the UI in sync with what discovery hears: loads one list, then follows the event stream the
 * shell publishes as servers appear and disappear.
 */
export function useNearbyServers(): NearbyServersController {
  const [servers, setServers] = useState<NearbyServer[]>([]);
  const [error, setError] = useState<AppError | null>(null);

  useEffect(() => {
    let active = true;
    let unsubscribe: UnlistenFn | undefined;

    const connect = async () => {
      try {
        const initial = await listNearbyServers();
        if (!active) {
          return;
        }
        setServers(initial.servers);
      } catch (cause) {
        if (!active) {
          return;
        }
        setError(toAppError(cause));
        return;
      }

      unsubscribe = await onNearbyServers((next) => {
        if (active) {
          setServers(next.servers);
        }
      });
    };

    void connect();

    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  const dismissError = useCallback(() => setError(null), []);

  return { servers, error, dismissError };
}
