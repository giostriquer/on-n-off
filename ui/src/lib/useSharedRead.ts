import { useEffect } from "react";
import { useQueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { SharedReadSource } from "$lib/types";

export const PENDING_RESET_SPENDS_KEY: QueryKey = ["reset-spends"];

const QUERY_KEYS: Record<SharedReadSource, readonly QueryKey[]> = {
  accounts: [["accounts"], ["subscription", "codex"]],
  "limits:claude": [["limits", "claude"]],
  "limits:codex": [["limits", "codex"]],
  "github:prs": [["github", "prs"]],
  "limits:reset-spends": [PENDING_RESET_SPENDS_KEY],
};

export function useSharedRead(source: SharedReadSource): void {
  const client = useQueryClient();
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void api
      .onSharedReadChanged((change) => {
        if (change.source !== source) return;
        for (const queryKey of QUERY_KEYS[source]) {
          void client.invalidateQueries({ queryKey });
        }
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
  }, [client, source]);
}
