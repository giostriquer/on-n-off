import { useEffect } from "react";
import { useQueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { SharedReadSource } from "$lib/types";

/**
 * The query each shared read backs. Keeping the mapping here rather than at the call sites is
 * what stops a source and a key drifting apart: a hook told to watch `limits:claude` while
 * refetching `["limits", "codex"]` would compile and then quietly never update either.
 */
const QUERY_KEYS: Record<SharedReadSource, QueryKey> = {
  "limits:claude": ["limits", "claude"],
  "limits:codex": ["limits", "codex"],
  "github:prs": ["github", "prs"],
};

/**
 * Refetches this source's query when the backend says the process-wide read behind it has been
 * replaced — by the other window, a monitor, or the notch's own poll.
 *
 * Without this a screen shows its own last answer until its poll interval comes round, however
 * recently something else fetched fresher numbers: refresh the Limits screen during a provider
 * outage that has just ended and the notch keeps the error for minutes, or the other way about.
 *
 * The refetch must stay unforced. It is then served from the same cache, so it costs no provider
 * call and replaces nothing, so it is not announced back; a forced one would call the provider,
 * replace the read, and announce again without end.
 */
export function useSharedRead(source: SharedReadSource): void {
  const client = useQueryClient();
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void api
      .onSharedReadChanged((change) => {
        if (change.source !== source) return;
        void client.invalidateQueries({ queryKey: QUERY_KEYS[source] });
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
