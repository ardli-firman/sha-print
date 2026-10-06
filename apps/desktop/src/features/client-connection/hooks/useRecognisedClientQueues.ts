import { useCallback, useEffect, useState } from "react";
import { listRecognisedClientQueues } from "../../../api/serverConnections";
import type { RecognisedClientQueue } from "../../../api/types";

export interface UseRecognisedClientQueues {
  queues: RecognisedClientQueue[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

export function useRecognisedClientQueues(): UseRecognisedClientQueues {
  const [queues, setQueues] = useState<RecognisedClientQueue[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await listRecognisedClientQueues();
      setQueues(result);
    } catch (err) {
      const message =
        err instanceof Error
          ? err.message
          : typeof err === "object" && err !== null && "message" in err
          ? String((err as { message: unknown }).message)
          : "Could not read installed client queues from Windows.";
      setError(message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return {
    queues,
    loading,
    error,
    refresh,
  };
}
