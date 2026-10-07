import { useCallback, useEffect, useState } from "react";

import {
  forgetTrustedServer,
  listTrustedServers,
  probeTrustedServer,
  type TrustedServer,
  type TrustedServerStatus,
} from "@/api/serverConnections";
import { toAppError, type AppError } from "@/api/types";

export interface TrustedServerEntry extends TrustedServer {
  status: TrustedServerStatus | "checking";
  current_fingerprint: string | null;
  printers: string[];
}

export interface TrustedServersController {
  servers: TrustedServerEntry[];
  loading: boolean;
  refreshing: Record<string, boolean>;
  forgetting: Record<string, boolean>;
  error: AppError | null;
  reload: () => Promise<void>;
  refresh: (address: string) => Promise<void>;
  forget: (address: string) => Promise<void>;
}

export function useTrustedServers(): TrustedServersController {
  const [servers, setServers] = useState<TrustedServerEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState<Record<string, boolean>>({});
  const [forgetting, setForgetting] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<AppError | null>(null);

  const applyProbe = useCallback((probe: Awaited<ReturnType<typeof probeTrustedServer>>) => {
    setServers((current) =>
      current.map((server) =>
        server.address === probe.address
          ? {
              ...server,
              fingerprint: probe.approved_fingerprint,
              status: probe.status,
              current_fingerprint: probe.current_fingerprint,
              printers: probe.printers,
            }
          : server,
      ),
    );
  }, []);

  const markOffline = useCallback((address: string) => {
    setServers((current) =>
      current.map((server) =>
        server.address === address
          ? { ...server, status: "offline", current_fingerprint: null, printers: [] }
          : server,
      ),
    );
  }, []);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const saved = await listTrustedServers();
      setServers(
        saved.map((server) => ({
          ...server,
          status: "checking",
          current_fingerprint: null,
          printers: [],
        })),
      );
      await Promise.all(
        saved.map(async (server) => {
          try {
            applyProbe(await probeTrustedServer(server.address));
          } catch (cause) {
            markOffline(server.address);
            setError(toAppError(cause));
          }
        }),
      );
    } catch (cause) {
      setServers([]);
      setError(toAppError(cause));
    } finally {
      setLoading(false);
    }
  }, [applyProbe, markOffline]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const refresh = useCallback(
    async (address: string) => {
      setRefreshing((current) => ({ ...current, [address]: true }));
      setError(null);
      try {
        applyProbe(await probeTrustedServer(address));
      } catch (cause) {
        const nextError = toAppError(cause);
        setError(nextError);
        markOffline(address);
        if (nextError.code === "server-untrusted") {
          await reload();
        }
      } finally {
        setRefreshing((current) => ({ ...current, [address]: false }));
      }
    },
    [applyProbe, markOffline, reload],
  );

  const forget = useCallback(async (address: string) => {
    setForgetting((current) => ({ ...current, [address]: true }));
    setError(null);
    try {
      await forgetTrustedServer(address);
      setServers((current) => current.filter((server) => server.address !== address));
    } catch (cause) {
      setError(toAppError(cause));
    } finally {
      setForgetting((current) => ({ ...current, [address]: false }));
    }
  }, []);

  return { servers, loading, refreshing, forgetting, error, reload, refresh, forget };
}
