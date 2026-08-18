import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { LimitsPopover } from "@/features/limits/LimitsPopover";
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
  if (surface === "limits-popover") {
    return (
      <QueryClientProvider client={popoverQueryClient}>
        <LimitsPopover />
      </QueryClientProvider>
    );
  }
  return <App />;
}
