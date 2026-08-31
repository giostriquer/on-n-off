import { useCallback, useEffect, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { NotchSettings, NotchSnapshot } from "$lib/notchTypes";

export function useNotchState() {
  const client = useQueryClient();
  const newestRevision = useRef(0);
  const acceptSnapshot = useCallback(
    async (snapshot: NotchSnapshot) => {
      const cached = client.getQueryData<NotchSnapshot>(["side-notch"]);
      const floor = Math.max(newestRevision.current, cached?.revision ?? 0);
      if (snapshot.revision < floor) return;
      newestRevision.current = snapshot.revision;
      // A read started before a save/event is no longer authoritative. Cancelling
      // the query also prevents its eventual IPC result from replacing this state.
      await client.cancelQueries({ queryKey: ["side-notch"] });
      if (snapshot.revision < newestRevision.current) return;
      client.setQueryData(["side-notch"], snapshot);
    },
    [client],
  );
  const query = useQuery({
    queryKey: ["side-notch"],
    queryFn: api.readNotchState,
    staleTime: 5_000,
  });
  const mutation = useMutation({
    mutationFn: (settings: NotchSettings) => api.saveNotchSettings(settings),
    onSuccess: acceptSnapshot,
  });
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void api
      .onNotchChanged(async (change) => {
        await acceptSnapshot(change.snapshot);
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
  }, [acceptSnapshot]);
  return {
    ...query,
    save: mutation.mutateAsync,
    saving: mutation.isPending,
    saveError: mutation.error,
  };
}
