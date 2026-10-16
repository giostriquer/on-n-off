import { useEffect, useState } from "react";
import { useQueries } from "@tanstack/react-query";
import { LIMITS_PROVIDERS, limitsQuery } from "$lib/sharedReads/limits";
import type { LimitsPollMinutes } from "$lib/types";

export function useLimitsProviders(pollMinutes: LimitsPollMinutes = 5) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const update = () => setNow(Date.now());
    const timer = window.setInterval(update, 60_000);
    window.addEventListener("focus", update);
    return () => { window.clearInterval(timer); window.removeEventListener("focus", update); };
  }, []);
  const queries = useQueries({ queries: LIMITS_PROVIDERS.map((provider) => limitsQuery(provider, pollMinutes)) });
  const providers = LIMITS_PROVIDERS.map((provider, index) => ({ provider, query: queries[index] }));
  const loading = queries.some((query) => query.isFetching);
  return { providers, loading, now };
}
