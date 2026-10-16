import { useEffect } from "react";
import { useQueryClient, type QueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { SharedReadSource } from "$lib/types";

export const PENDING_RESET_SPENDS_KEY: QueryKey = ["reset-spends"];

const QUERY_KEYS: Record<SharedReadSource, readonly QueryKey[]> = {
  accounts: [["accounts"], ["subscription", "codex"]],
  "limits:claude": [["limits", "claude"]],
  "limits:codex": [["limits", "codex"]],
  "limits:muse": [["limits", "muse"]],
  "github:prs": [["github", "prs"]],
  "limits:reset-spends": [PENDING_RESET_SPENDS_KEY],
};

export function connectSharedReads(client: QueryClient): () => void {
  let disposed = false;
  let stop: (() => void) | undefined;
  void api
    .onSharedReadChanged((change) => {
      for (const queryKey of QUERY_KEYS[change.source] ?? []) {
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
}

export function SharedReads(): null {
  const client = useQueryClient();
  useEffect(() => connectSharedReads(client), [client]);
  return null;
}
