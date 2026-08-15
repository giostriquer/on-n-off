import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { SessionProvider, useAgentSession } from "@/features/session/SessionProvider";
import { UpdateProvider } from "@/features/updater/UpdateProvider";
import type { ReactNode } from "react";
import { router } from "./router";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      refetchOnWindowFocus: false,
    },
  },
});

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <SessionProvider>
        <SessionUpdates>
          <RouterProvider router={router} />
        </SessionUpdates>
      </SessionProvider>
    </QueryClientProvider>
  );
}

function SessionUpdates({ children }: { children: ReactNode }) {
  const session = useAgentSession();
  return (
    <UpdateProvider
      initialProviderReady={session.initialProviderReady}
      automaticUpdates={session.appSettings.automaticUpdates}
    >
      {children}
    </UpdateProvider>
  );
}
