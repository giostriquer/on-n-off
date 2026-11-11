import type { DesktopResets } from "$lib/limitsTypes";
import { providerLabel, type LimitsProvider } from "$lib/providers";
import { unexpiredBankedResets } from "./limitPresentation";

export type DesktopPresentation = {
  name: string;
  label: string;
  tone: "neutral";
  progress: 0;
  headline: string;
};

export type DesktopResetsPresentation = DesktopResets & { app: string };

function desktopApp(provider: LimitsProvider): string {
  return `${providerLabel(provider)} Desktop`;
}

export function desktopResetsPresentation(provider: LimitsProvider, seen: DesktopResets | null | undefined, now: number): DesktopResetsPresentation | null {
  return seen && unexpiredBankedResets(seen, now) ? { ...seen, app: desktopApp(provider) } : null;
}

export function desktopPresentation(provider: LimitsProvider): DesktopPresentation {
  const app = desktopApp(provider);
  return {
    name: `${app} account`, label: "Desktop", tone: "neutral", progress: 0,
    headline: `${app} was last signed in to this account.`,
  };
}
