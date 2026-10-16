import { QueryClientProvider } from "@tanstack/react-query";
import { lazy, Suspense, useLayoutEffect } from "react";
const LimitsPopover = lazy(() => import("@/features/limits/LimitsPopover").then(module => ({ default: module.LimitsPopover })));
import { createPopoverQueryClient } from "$lib/queryClients";
import { AppSettingsProvider } from "$lib/useAppSettings";
import { SharedReads } from "$lib/sharedReads/announcements";
import { App } from "./App";

const popoverQueryClient = createPopoverQueryClient();

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
        <SharedReads />
        <AppSettingsProvider>
          <Suspense fallback={null}><LimitsPopover /></Suspense>
        </AppSettingsProvider>
      </QueryClientProvider>
    );
  }
  return <App />;
}
