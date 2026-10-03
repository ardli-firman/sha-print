import { useCallback, useEffect, useState } from "react";

import {
  getLegacyImportReport,
  importLegacySettings,
  onLegacyImport,
  type LegacyImportReport,
} from "@/api/legacyImport";
import type { UnlistenFn } from "@/api/ipc";
import { toAppError, type AppError } from "@/api/types";

/** What the previous-app panel needs to render and re-run the import. */
export interface LegacyImportController {
  /** Latest report, or `null` until the first response arrives. */
  report: LegacyImportReport | null;
  /** Most recent failure worth showing the user. */
  error: AppError | null;
  /** True while a re-run is in flight. */
  busy: boolean;
  checkAgain: () => Promise<void>;
}

/**
 * Keeps the window in sync with the import from the previous app. The shell runs it while starting,
 * so this reads the current result and then follows the event, which is what turns a report from
 * "ready to import" into "imported" without a reload.
 */
export function useLegacyImport(): LegacyImportController {
  const [report, setReport] = useState<LegacyImportReport | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let active = true;
    let unsubscribe: UnlistenFn | undefined;

    const connect = async () => {
      try {
        const initial = await getLegacyImportReport();
        if (!active) {
          return;
        }
        setReport(initial);
      } catch (cause) {
        if (!active) {
          return;
        }
        setError(toAppError(cause));
        return;
      }

      unsubscribe = await onLegacyImport((next) => {
        if (active) {
          setReport(next);
        }
      });
    };

    void connect();

    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  const checkAgain = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setReport(await importLegacySettings());
    } catch (cause) {
      setError(toAppError(cause));
    } finally {
      setBusy(false);
    }
  }, []);

  return { report, error, busy, checkAgain };
}
