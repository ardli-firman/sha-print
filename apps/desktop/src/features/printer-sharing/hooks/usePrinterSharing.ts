import { useCallback, useEffect, useState } from "react";

import {
  getServerIdentity,
  listLocalPrinters,
  setSharedPrinters,
} from "@/api/ipc";
import {
  toAppError,
  type AppError,
  type LocalPrinter,
  type ServerIdentity,
} from "@/api/types";

/** What the sharing panel needs to render and change the server's shared printers. */
export interface PrinterSharingController {
  /** Local queues and their sharing state, or `null` until the first response arrives. */
  printers: LocalPrinter[] | null;
  /** The identity clients approve and the port they connect to. */
  identity: ServerIdentity | null;
  /** Queue whose sharing state is being changed, if any. */
  busy: string | null;
  /** Most recent failure worth showing the user. */
  error: AppError | null;
  toggle: (name: string) => Promise<void>;
  refresh: () => Promise<void>;
  dismissError: () => void;
}

/**
 * Keeps the sharing panel in sync with the shell: it lists the local queues and the server
 * identity, and every change returns the state the shell actually applied. A rejected change
 * re-reads the queues, so a checkbox never shows a state the server is not in — and keeps the
 * failure visible while it does.
 */
export function usePrinterSharing(): PrinterSharingController {
  const [printers, setPrinters] = useState<LocalPrinter[] | null>(null);
  const [identity, setIdentity] = useState<ServerIdentity | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  const load = useCallback(async () => {
    try {
      const [listing, serverIdentity] = await Promise.all([
        listLocalPrinters(),
        getServerIdentity(),
      ]);
      setPrinters(listing.printers);
      setIdentity(serverIdentity);
    } catch (cause) {
      setError(toAppError(cause));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const toggle = useCallback(
    async (name: string) => {
      setBusy(name);
      setError(null);
      try {
        const next = (printers ?? [])
          .filter((printer) => (printer.name === name ? !printer.shared : printer.shared))
          .map((printer) => printer.name);
        const updated = await setSharedPrinters(next);
        setPrinters(updated.printers);
      } catch (cause) {
        setError(toAppError(cause));
        await load();
      } finally {
        setBusy(null);
      }
    },
    [load, printers],
  );

  const dismissError = useCallback(() => {
    setError(null);
  }, []);

  return {
    printers,
    identity,
    busy,
    error,
    toggle,
    refresh: load,
    dismissError,
  };
}
