import { useCallback, useEffect, useState } from "react";

import { getStartupStatus, setStartupEnabled, type StartupStatus } from "@/api/startup";
import { toAppError, type AppError } from "@/api/types";

/** What the startup panel needs to render and change the login registration. */
export interface StartupController {
  /** Current registration, or `null` until the first response arrives. */
  status: StartupStatus | null;
  /** Most recent failure worth showing the user. */
  error: AppError | null;
  /** True while a change is in flight. */
  busy: boolean;
  setEnabled: (enabled: boolean) => Promise<void>;
}

/** Reads the login registration once and reports the outcome of every change. */
export function useStartup(): StartupController {
  const [status, setStatus] = useState<StartupStatus | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let active = true;

    const load = async () => {
      try {
        const initial = await getStartupStatus();
        if (active) {
          setStatus(initial);
        }
      } catch (cause) {
        if (active) {
          setError(toAppError(cause));
        }
      }
    };

    void load();

    return () => {
      active = false;
    };
  }, []);

  const setEnabled = useCallback(async (enabled: boolean) => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await setStartupEnabled(enabled));
    } catch (cause) {
      setError(toAppError(cause));
      try {
        setStatus(await getStartupStatus());
      } catch {
        // Keep the last known setting; the banner already explains the failure.
      }
    } finally {
      setBusy(false);
    }
  }, []);

  return { status, error, busy, setEnabled };
}
