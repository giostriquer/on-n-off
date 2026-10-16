import type { QueryClient, QueryKey } from "@tanstack/react-query";

export async function readForced<T>(
  client: QueryClient,
  queryKey: QueryKey,
  read: (force: boolean) => Promise<T>,
): Promise<T> {
  let running = client.getQueryCache().find({ queryKey, exact: true });
  while (running?.state.fetchStatus === "fetching" && running.promise) {
    await running.promise.catch(() => undefined);
    running = client.getQueryCache().find({ queryKey, exact: true });
  }
  let armed = true;
  return client.fetchQuery({
    queryKey,
    staleTime: 0,
    queryFn: () => {
      const force = armed;
      armed = false;
      return read(force);
    },
  });
}
