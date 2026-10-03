import { useCallback, useEffect, useState } from "react";

import {
  getRuntimeStatus,
  onRuntimeStatus,
  startService,
  stopService,
  type ServiceAction,
  type UnlistenFn,
} from "@/api/ipc";
import { toAppError, type AppError, type RuntimeStatus, type ServiceId } from "@/api/types";

/** What the status panel needs to render and operate the supervised services. */
export interface RuntimeStatusController {
  /** Latest snapshot, or `null` until the first response arrives. */
  status: RuntimeStatus | null;
  /** Most recent failure worth showing the user. */
  error: AppError | null;
  /** Service with an action in flight, if any. */
  busy: ServiceId | null;
  /** True when the shell's runtime cannot be reached at all. */
  unreachable: boolean;
  start: (id: ServiceId) => Promise<void>;
  stop: (id: ServiceId) => Promise<void>;
  refresh: () => Promise<void>;
  dismissError: () => void;
}

/**
 * Keeps the UI in sync with the shell: loads one snapshot, then follows the status event stream.
 * Actions return a fresh snapshot, and a failed action re-reads the status so the UI never shows
 * a state the runtime is not actually in.
 */
export function useRuntimeStatus(): RuntimeStatusController {
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState<ServiceId | null>(null);
  const [unreachable, setUnreachable] = useState(false);

  useEffect(() => {
    let active = true;
    let unsubscribe: UnlistenFn | undefined;

    const connect = async () => {
      try {
        const initial = await getRuntimeStatus();
        if (!active) {
          return;
        }
        setStatus(initial);
        setUnreachable(false);
      } catch (cause) {
        if (!active) {
          return;
        }
        setUnreachable(true);
        setError(toAppError(cause));
        return;
      }

      unsubscribe = await onRuntimeStatus((next) => {
        if (active) {
          setStatus(next);
        }
      });
    };

    void connect();

    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  const reload = useCallback(async () => {
    try {
      setStatus(await getRuntimeStatus());
      setUnreachable(false);
    } catch (cause) {
      setUnreachable(true);
      setError(toAppError(cause));
    }
  }, []);

  const act = useCallback(async (id: ServiceId, action: ServiceAction) => {
    setBusy(id);
    setError(null);
    try {
      setStatus(await action(id));
    } catch (cause) {
      setError(toAppError(cause));
      try {
        setStatus(await getRuntimeStatus());
      } catch {
        // Keep the last known snapshot; the error banner already explains the failure.
      }
    } finally {
      setBusy(null);
    }
  }, []);

  const start = useCallback((id: ServiceId) => act(id, startService), [act]);
  const stop = useCallback((id: ServiceId) => act(id, stopService), [act]);
  const dismissError = useCallback(() => setError(null), []);

  return {
    status,
    error,
    busy,
    unreachable,
    start,
    stop,
    refresh: reload,
    dismissError,
  };
}
