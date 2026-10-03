import { useCallback, useEffect, useState } from "react";

import { configureNetworkChannel, getNetworkChannelStatus } from "@/api/networkChannel";
import { toAppError, type AppError } from "@/api/types";

export interface NetworkChannelController {
  configured: boolean | null;
  saving: boolean;
  error: AppError | null;
  save: (networkChannel: string) => Promise<boolean>;
  refresh: () => Promise<void>;
}

/** Loads only safe configured state and never keeps a saved secret in component state. */
export function useNetworkChannel(): NetworkChannelController {
  const [configured, setConfigured] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  const refresh = useCallback(async () => {
    try {
      setConfigured(await getNetworkChannelStatus());
    } catch (cause) {
      setError(toAppError(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = useCallback(async (networkChannel: string) => {
    setSaving(true);
    setError(null);
    try {
      const nextConfigured = await configureNetworkChannel(networkChannel);
      setConfigured(nextConfigured);
      return true;
    } catch (cause) {
      setError(toAppError(cause));
      return false;
    } finally {
      setSaving(false);
    }
  }, []);

  return { configured, saving, error, save, refresh };
}
