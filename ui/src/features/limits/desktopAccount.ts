import { providerLabel, type LimitsProvider } from "$lib/providers";

export type DesktopPresentation = {
  name: string;
  label: string;
  tone: "neutral";
  progress: 0;
  headline: string;
};

export function desktopPresentation(provider: LimitsProvider): DesktopPresentation {
  const app = `${providerLabel(provider)} Desktop`;
  return {
    name: `${app} account`, label: "Desktop", tone: "neutral", progress: 0,
    headline: `${app} was last signed in to this account.`,
  };
}
