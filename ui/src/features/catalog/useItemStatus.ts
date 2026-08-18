import { keepPreviousData, useQueries, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo, useRef } from "react";
import * as api from "$lib/api";
import { ITEM_STATUS_KEY, type ItemStatusSets } from "$lib/itemStatus";
import type { AgentId, ItemStatus, UpdateItemMode } from "$lib/types";

const ITEM_STATUS_STALE_MS = 5 * 60_000;

/**
 * Managed-item statuses for a provider: always the global set, plus the project set when a
 * project is selected. `refresh()` forces the upstream check (one GitHub request per repo).
 */
export function useItemStatus(provider: AgentId, projectPath: string | null) {
  const forceRef = useRef(false);
  const client = useQueryClient();
  const scopes: (string | null)[] = projectPath ? [null, projectPath] : [null];
  const queries = useQueries({
    queries: scopes.map((path) => ({
      queryKey: [ITEM_STATUS_KEY, provider, path ?? ""],
      queryFn: () => {
        const force = forceRef.current;
        return api.itemUpdateStatus(provider, path, force);
      },
      staleTime: ITEM_STATUS_STALE_MS,
      placeholderData: keepPreviousData,
      retry: false,
    })),
  });
  const globalData = queries[0]?.data;
  const projectData = queries[1]?.data;
  const sets: ItemStatusSets = useMemo(
    () => ({ global: globalData ?? [], project: projectData ?? [] }),
    [globalData, projectData],
  );
  const fetching = queries.some((query) => query.isFetching);

  const refresh = useCallback(async () => {
    forceRef.current = true;
    try {
      await client.refetchQueries({ queryKey: [ITEM_STATUS_KEY, provider] });
    } finally {
      forceRef.current = false;
    }
  }, [client, provider]);

  return { sets, fetching, refresh };
}

/** Update / dismiss / remove one managed item, then refresh statuses and the provider's tab. */
export function useItemActions(provider: AgentId, afterChange: () => Promise<unknown> | void) {
  const client = useQueryClient();
  const settle = useCallback(async () => {
    await client.invalidateQueries({ queryKey: [ITEM_STATUS_KEY, provider] });
    await afterChange();
  }, [afterChange, client, provider]);

  const update = useCallback(
    async (status: ItemStatus, mode: UpdateItemMode) => {
      await api.updateItem(status.id, mode);
      await settle();
    },
    [settle],
  );

  const remove = useCallback(
    async (status: ItemStatus) => {
      await api.removeItem(status.id);
      await settle();
    },
    [settle],
  );

  return { update, remove };
}
