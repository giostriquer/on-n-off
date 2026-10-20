import type { OtherDesktopAccount } from "$lib/limitsTypes";
import { providerLabel, type LimitsProvider } from "$lib/providers";

export type DesktopPresentation = {
  name: string;
  label: string;
  tone: "warning";
  progress: number;
  headline: string;
  next: string;
};

export function desktopPresentation(other: OtherDesktopAccount | undefined, provider: LimitsProvider): DesktopPresentation | null {
  if (!other) return null;
  const app = `${providerLabel(provider)} Desktop`;
  return {
    name: `${app} is signed in to another account`, label: "Desktop: other account", tone: "warning", progress: 0,
    headline: `${app} is signed in to ${other.label ?? "another account"}.`,
    next: `Sign in to this account in ${app} to use it there.`,
  };
}
