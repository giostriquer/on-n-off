import { useEffect } from "react";
import { useQueryClient, type QueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { SharedReadSource } from "$lib/types";
import { accountsAnnouncements } from "./accounts";
import { githubPrsAnnouncements } from "./githubPrs";
import { limitsAnnouncements } from "./limits";
import { resetSpendsAnnouncements } from "./resetSpends";

export const SHARED_READ_KEYS = {
  ...accountsAnnouncements,
  ...limitsAnnouncements,
  ...githubPrsAnnouncements,
  ...resetSpendsAnnouncements,
} satisfies Record<SharedReadSource, readonly QueryKey[]>;

export function connectSharedReads(client: QueryClient): () => void {
  let disposed = false;
  let stop: (() => void) | undefined;
  void api
    .onSharedReadChanged((change) => {
      const keys: readonly QueryKey[] = SHARED_READ_KEYS[change.source] ?? [];
      for (const queryKey of keys) {
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
