import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { SessionProvider, useAgentSession } from "@/features/session/SessionProvider";
import { UpdateProvider } from "@/features/updater/UpdateProvider";
import { AppSettingsProvider, useAppSettings } from "$lib/useAppSettings";
import { SharedReads } from "$lib/useSharedRead";
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
      <SharedReads />
      <AppSettingsProvider>
        <SessionProvider>
          <SessionUpdates>
            <RouterProvider router={router} />
          </SessionUpdates>
        </SessionProvider>
      </AppSettingsProvider>
    </QueryClientProvider>
  );
}

function SessionUpdates({ children }: { children: ReactNode }) {
  const session = useAgentSession();
  const { settings } = useAppSettings();
  return (
    <UpdateProvider
      initialProviderReady={session.initialProviderReady}
      automaticUpdates={settings.automaticUpdates}
    >
      {children}
    </UpdateProvider>
  );
}
