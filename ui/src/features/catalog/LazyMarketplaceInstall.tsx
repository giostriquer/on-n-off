import { lazy, Suspense } from "react";
import { copy } from "$lib/copy";
import type { MarketplaceInstallProps } from "./MarketplaceInstall";

type MarketplaceInstallModule = typeof import("./MarketplaceInstall");

let panelPromise: Promise<MarketplaceInstallModule> | undefined;

export function preloadMarketplaceInstall(): Promise<MarketplaceInstallModule> {
  panelPromise ??= import("./MarketplaceInstall");
  return panelPromise;
}

const MarketplaceInstall = lazy(() =>
  preloadMarketplaceInstall().then((module) => ({ default: module.MarketplaceInstall })),
);

export function LazyMarketplaceInstall(props: MarketplaceInstallProps) {
  return (
    <Suspense
      fallback={
        <>
          <p className="text-[11.5px] text-[var(--mute)]">{copy.marketplaceLoading}</p>
          <footer className="mt-1 flex justify-end gap-2">
            <button
              type="button"
              className="h-8 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-3.5 text-[12.5px] text-[var(--silkscreen)]"
              onClick={props.onCancel}
            >
              {copy.cancel}
            </button>
          </footer>
        </>
      }
    >
      <MarketplaceInstall {...props} />
    </Suspense>
  );
}
