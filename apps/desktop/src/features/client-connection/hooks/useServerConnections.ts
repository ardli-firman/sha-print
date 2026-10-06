import { useCallback, useState } from "react";

import {
  approveServerConnection,
  installPrinterQueue,
  inspectServerConnection,
  listServerConnectionPrinters,
  type ServerConnectionPrinters,
  type ServerConnectionReview,
} from "@/api/serverConnections";
import { toAppError, type AppError, type ClientQueue } from "@/api/types";

export interface ServerConnectionsController {
  address: string;
  setAddress: (value: string) => void;
  review: ServerConnectionReview | null;
  result: ServerConnectionPrinters | null;
  /** The queue installed for a shared printer, once one has been installed. */
  installed: ClientQueue | null;
  busy: "inspect" | "approve" | "printers" | "install" | null;
  error: AppError | null;
  inspect: () => Promise<void>;
  /**
   * Reviews `value` and shows it in the address field. Used for an address the user did not type,
   * such as one discovery reported.
   */
  reviewAddress: (value: string) => Promise<void>;
  approve: () => Promise<void>;
  query: () => Promise<void>;
  install: (printer: string) => Promise<void>;
  reset: () => void;
}

export function useServerConnections(): ServerConnectionsController {
  const [address, setAddress] = useState("");
  const [review, setReview] = useState<ServerConnectionReview | null>(null);
  const [result, setResult] = useState<ServerConnectionPrinters | null>(null);
  const [installed, setInstalled] = useState<ClientQueue | null>(null);
  const [busy, setBusy] = useState<ServerConnectionsController["busy"]>(null);
  const [error, setError] = useState<AppError | null>(null);

  const run = useCallback(
    async <T,>(
      kind: NonNullable<ServerConnectionsController["busy"]>,
      operation: () => Promise<T>,
      success: (value: T) => void,
    ) => {
      setBusy(kind);
      setError(null);
      try {
        success(await operation());
      } catch (cause) {
        setError(toAppError(cause));
      } finally {
        setBusy(null);
      }
    },
    [],
  );

  const inspect = useCallback(() => {
    setReview(null);
    setResult(null);
    setInstalled(null);
    return run("inspect", () => inspectServerConnection(address), setReview);
  }, [address, run]);

  const reviewAddress = useCallback(
    (value: string) => {
      setAddress(value);
      setReview(null);
      setResult(null);
      setInstalled(null);
      setError(null);
      return run("inspect", () => inspectServerConnection(value), setReview);
    },
    [run],
  );

  const approve = useCallback(() => {
    const current = review;
    if (!current) return Promise.resolve();
    return run(
      "approve",
      () => approveServerConnection(current.address, current.current_fingerprint),
      (value) => {
        setReview(value);
        setResult(null);
        setInstalled(null);
      },
    );
  }, [review, run]);

  const query = useCallback(() => {
    const current = review;
    if (!current?.trusted) return Promise.resolve();
    return run("printers", () => listServerConnectionPrinters(current.address), setResult);
  }, [review, run]);

  // A queue is installed only for an already-approved server; the shell checks that the server
  // still shares the printer before it asks for administrator permission.
  const install = useCallback(
    (printer: string) => {
      const current = review;
      if (!current?.trusted) return Promise.resolve();
      return run("install", () => installPrinterQueue(current.address, printer), setInstalled);
    },
    [review, run],
  );

  const updateAddress = useCallback((value: string) => {
    setAddress(value);
    setReview(null);
    setResult(null);
    setInstalled(null);
    setError(null);
  }, []);

  const reset = useCallback(() => {
    setAddress("");
    setReview(null);
    setResult(null);
    setInstalled(null);
    setError(null);
    setBusy(null);
  }, []);

  return {
    address,
    setAddress: updateAddress,
    review,
    result,
    installed,
    busy,
    error,
    inspect,
    reviewAddress,
    approve,
    query,
    install,
    reset,
  };
}
