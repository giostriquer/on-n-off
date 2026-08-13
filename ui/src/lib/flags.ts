import type { FeatureFlags } from "./types";

export const DEFAULT_FLAGS: FeatureFlags = {
  masterCut: false,
};

export function mergeFlags(overlay: Partial<FeatureFlags> | null | undefined): FeatureFlags {
  return {
    ...DEFAULT_FLAGS,
    ...overlay,
  };
}

export function flagOn(flags: FeatureFlags, name: keyof FeatureFlags): boolean {
  return flags[name] === true;
}
