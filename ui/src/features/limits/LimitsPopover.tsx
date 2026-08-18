import { useEffect, useLayoutEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import { formatClock } from "$lib/limitsFormat";
import { applyStoredTheme } from "$lib/theme";
import { ProviderColumn } from "./Limits";
import { LIMITS_STALE_MS, useLimitsProviders } from "./useLimitsProviders";

export function LimitsPopover() {
  const queryClient = useQueryClient();
  const [actionError, setActionError] = useState<string | null>(null);
  const { providers, loading, asOf, now, refresh } = useLimitsProviders();
  const [claude, codex] = providers;

  useLayoutEffect(() => {
    applyStoredTheme();
  }, []);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        void api.hideLimitsPopover();
      }
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void api
      .onLimitsPopoverOpened(() => {
        applyStoredTheme();
        void queryClient.refetchQueries({
          queryKey: ["limits"],
          type: "active",
          predicate: (query) =>
            query.state.dataUpdatedAt > 0 && Date.now() - query.state.dataUpdatedAt >= LIMITS_STALE_MS,
        });
      })
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);

  async function runAction(label: string, action: () => Promise<void>) {
    setActionError(null);
    try {
      await action();
    } catch (reason) {
      setActionError(`Could not ${label}: ${displayError(parseInvokeError(reason), "on-n-off")}`);
    }
  }

  return (
    <main className="flex h-full min-h-0 flex-col bg-[var(--void)] text-[var(--silkscreen)]">
      <header className="flex shrink-0 items-center gap-3 border-b border-[var(--hair)] px-3 py-2.5">
        <h1 className="min-w-0 flex-1 font-mono text-[12px] text-[var(--mute)]">
          Subscription limits{asOf ? ` · as of ${formatClock(asOf)}` : null}
        </h1>
        <button
          type="button"
          className="inline-flex size-7 shrink-0 items-center justify-center rounded-md border border-[var(--hair)] bg-[var(--plate)] text-[var(--mute)] disabled:opacity-45"
          disabled={loading}
          aria-label="Refresh limits"
          onClick={refresh}
        >
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} aria-hidden="true" />
        </button>
      </header>
      <div className="limits-popover-scroll flex min-h-0 flex-1 flex-col gap-3 p-3">
        <ProviderColumn provider={claude.provider} query={claude.query} now={now} allowForget={false} />
        <ProviderColumn provider={codex.provider} query={codex.query} now={now} allowForget={false} />
      </div>
      {actionError ? (
        <p className="shrink-0 border-t border-[var(--hair)] px-3 pt-2 text-[11.5px] text-[var(--trip)]" role="alert">
          {actionError}
        </p>
      ) : null}
      <footer className="flex shrink-0 items-center justify-between border-t border-[var(--hair)] px-3 py-2.5">
        <button
          type="button"
          className="border-0 bg-transparent px-1 py-1.5 text-[11.5px] font-semibold text-[var(--mute)]"
          onClick={() => void runAction("quit on-n-off", api.quitApp)}
        >
          Quit
        </button>
        <button
          type="button"
          className="rounded-md bg-[var(--fill)] px-3 py-1.5 text-[11.5px] font-semibold text-[var(--fill-ink)]"
          onClick={() => void runAction("open on-n-off", api.openLimitsWindow)}
        >
          Open on-n-off
        </button>
      </footer>
    </main>
  );
}
