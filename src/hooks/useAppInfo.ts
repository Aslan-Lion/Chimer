import { useCallback, useEffect, useState } from "react";
import type { AppInfo } from "../services/backend";
import { getAppInfo } from "../services/backend";

interface AppInfoResult {
  info: AppInfo | null;
  error: string | null;
}

/** Loads application metadata from Rust and surfaces failures instead of hiding them. */
export function useAppInfo(): AppInfoResult {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    getAppInfo()
      .then((result) => {
        if (!cancelled) setInfo(result);
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
      });

    return () => {
      cancelled = true;
    };
  }, []);

  return { info, error };
}

export function useAppInfoRefresher(): () => Promise<void> {
  const [, force] = useState(0);
  return useCallback(async () => {
    await getAppInfo();
    force((value) => value + 1);
  }, []);
}
