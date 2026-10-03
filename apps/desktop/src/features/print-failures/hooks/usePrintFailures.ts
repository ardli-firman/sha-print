import { useCallback, useEffect, useState } from "react";

import {
  dismissPrintFailure,
  getPrintFailures,
  onPrintFailure,
} from "@/api/printFailures";
import type { UnlistenFn } from "@/api/ipc";
import { toAppError, type AppError, type PrintFailure } from "@/api/types";

/** What the print-failure panel needs to render and clear the latest failure. */
export interface PrintFailuresController {
  /** Latest failure, or `null` when nothing has failed since the last dismissal. */
  failure: PrintFailure | null;
  /** Most recent failure of the dismiss action itself. */
  error: AppError | null;
  dismiss: () => Promise<void>;
}

/**
 * Keeps the window in sync with the shell's print-failure surface: one snapshot on mount, then the
 * event stream, so a job that fails while the window is open appears immediately.
 */
export function usePrintFailures(): PrintFailuresController {
  const [failure, setFailure] = useState<PrintFailure | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  useEffect(() => {
    let active = true;
    let unsubscribe: UnlistenFn | undefined;

    const connect = async () => {
      try {
        const initial = await getPrintFailures();
        if (!active) {
          return;
        }
        setFailure(initial);
      } catch (cause) {
        if (!active) {
          return;
        }
        setError(toAppError(cause));
        return;
      }

      unsubscribe = await onPrintFailure((next) => {
        if (active) {
          setFailure(next);
        }
      });
    };

    void connect();

    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  const dismiss = useCallback(async () => {
    setError(null);
    try {
      const next = await dismissPrintFailure();
      setFailure(next);
    } catch (cause) {
      setError(toAppError(cause));
    }
  }, []);

  return { failure, error, dismiss };
}
