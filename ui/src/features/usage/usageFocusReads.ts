import { useEffect } from "react";
import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";

export const USAGE_QUERY_ROOT = ["usage"] as const;

export function connectUsageFocusReads(client: QueryClient): () => void {
  let disposed = false;
  let stop: (() => void) | undefined;
  void api
    .onMainWindowFocused(() => {
      const staleTime = client.getDefaultOptions().queries?.staleTime;
      void client.refetchQueries(
        {
          queryKey: USAGE_QUERY_ROOT,
          type: "active",
          predicate: (query) => query.isStaleByTime(typeof staleTime === "function" ? staleTime(query) : staleTime),
        },
        { cancelRefetch: false },
      );
    })
    .then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    })
    .catch(() => undefined);
  return () => {
    disposed = true;
    stop?.();
  };
}

export function UsageFocusReads(): null {
  const client = useQueryClient();
  useEffect(() => connectUsageFocusReads(client), [client]);
  return null;
}
