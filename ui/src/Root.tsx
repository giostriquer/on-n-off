import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { lazy, Suspense, useLayoutEffect } from "react";
const LimitsPopover = lazy(() => import("@/features/limits/LimitsPopover").then(module => ({ default: module.LimitsPopover })));
import { App } from "./App";

const popoverQueryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
    },
  },
});

export function Root({ search = window.location.search }: { search?: string }) {
  const surface = new URLSearchParams(search).get("surface");

  useLayoutEffect(() => {
    if (surface === "limits-popover") {
      document.documentElement.dataset.surface = surface;
    } else {
      delete document.documentElement.dataset.surface;
    }
    return () => {
      delete document.documentElement.dataset.surface;
    };
  }, [surface]);

  if (surface === "limits-popover") {
    return (
      <QueryClientProvider client={popoverQueryClient}>
        <Suspense fallback={null}><LimitsPopover /></Suspense>
      </QueryClientProvider>
    );
  }
  return <App />;
}
