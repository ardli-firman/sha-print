import { useCallback, useEffect, useState } from "react";

import {
  applyUpdateAndRestart,
  checkForUpdates,
  getUpdateStatus,
  onUpdateStatus,
  type UpdateStatus,
} from "@/api/updates";

export function useUpdates() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void onUpdateStatus((next) => {
      if (active) {
        setStatus(next);
        setError(next.update.state === "failed" ? next.update.message : null);
      }
    }).then((stop) => {
      unsubscribe = stop;
      if (!active) stop();
    });
    void getUpdateStatus().then((next) => {
      if (active) setStatus(next);
    }).catch(() => {
      // The desktop shell may still be starting; the next status event will populate this surface.
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  const check = useCallback(async () => {
    setChecking(true);
    setError(null);
    try {
      setStatus(await checkForUpdates());
    } catch (cause) {
      const message = typeof cause === "object" && cause !== null && "message" in cause
        ? String((cause as { message: unknown }).message)
        : "ShaPrint could not check for updates. Check your internet connection and try again.";
      setError(message);
    } finally {
      setChecking(false);
    }
  }, []);

  const restart = useCallback(async () => {
    setError(null);
    try {
      setStatus(await applyUpdateAndRestart());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "ShaPrint could not prepare the restart.");
    }
  }, []);

  return { status, checking, error, check, restart };
}
