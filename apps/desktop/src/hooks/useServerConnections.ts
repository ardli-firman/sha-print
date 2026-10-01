import { useCallback, useState } from "react";
import { approveServerConnection, inspectServerConnection, listServerConnectionPrinters, type ServerConnectionPrinters, type ServerConnectionReview } from "../api/serverConnections";
import { toAppError, type AppError } from "../api/types";

export interface ServerConnectionsController {
  address: string;
  setAddress: (value: string) => void;
  review: ServerConnectionReview | null;
  result: ServerConnectionPrinters | null;
  busy: "inspect" | "approve" | "printers" | null;
  error: AppError | null;
  inspect: () => Promise<void>;
  approve: () => Promise<void>;
  query: () => Promise<void>;
}
export function useServerConnections(): ServerConnectionsController {
  const [address, setAddress] = useState("");
  const [review, setReview] = useState<ServerConnectionReview | null>(null);
  const [result, setResult] = useState<ServerConnectionPrinters | null>(null);
  const [busy, setBusy] = useState<ServerConnectionsController["busy"]>(null);
  const [error, setError] = useState<AppError | null>(null);
  const run = useCallback(async <T,>(kind: NonNullable<ServerConnectionsController["busy"]>, operation: () => Promise<T>, success: (value: T) => void) => {
    setBusy(kind); setError(null);
    try { success(await operation()); } catch (cause) { setError(toAppError(cause)); } finally { setBusy(null); }
  }, []);
  const inspect = useCallback(() => { setReview(null); setResult(null); return run("inspect", () => inspectServerConnection(address), setReview); }, [address, run]);
  const approve = useCallback(() => {
    const current = review;
    if (!current) return Promise.resolve();
    return run("approve", () => approveServerConnection(current.address, current.current_fingerprint), value => { setReview(value); setResult(null); });
  }, [review, run]);
  const query = useCallback(() => {
    const current = review;
    if (!current?.trusted) return Promise.resolve();
    return run("printers", () => listServerConnectionPrinters(current.address), setResult);
  }, [review, run]);
  const updateAddress = useCallback((value: string) => { setAddress(value); setReview(null); setResult(null); setError(null); }, []);
  return { address, setAddress: updateAddress, review, result, busy, error, inspect, approve, query };
}
