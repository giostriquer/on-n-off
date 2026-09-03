import { useEffect, useRef } from "react";
import { useQueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { SharedReadSource } from "$lib/types";

/**
 * Refetches a query when the backend says the process-wide read behind it has been replaced —
 * by the other window, a monitor, or the notch's own poll.
 *
 * Without this a screen shows its own last answer until its poll interval comes round, however
 * recently something else fetched fresher numbers: refresh the Limits screen during a provider
 * outage that has just ended and the notch keeps the error for minutes, or the other way about.
 * The refetch this provokes is served from that same cache, so it costs no provider call, and
 * carries a revision already announced, so it announces nothing back.
 */
export function useSharedRead(source: SharedReadSource, queryKey: QueryKey): void {
  const client = useQueryClient();
  // Read at announcement time, so a caller may build the key inline without re-subscribing.
  const key = useRef(queryKey);
  key.current = queryKey;
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void api
      .onSharedReadChanged((change) => {
        if (change.source !== source) return;
        void client.invalidateQueries({ queryKey: key.current });
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
