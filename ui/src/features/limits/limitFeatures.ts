import { LIMITS_FEATURE_ROWS, LIMITS_PROVIDERS, type LimitsProvider } from "$lib/providers";

export const LIMITS_IMPLEMENTATIONS = {
  signInDeadline: ["claude"],
  subscriptionTerm: ["codex"],
  bankedResets: ["codex"],
  paidResetOffer: ["codex"],
} as const;

type Feature = keyof typeof LIMITS_IMPLEMENTATIONS;

export type LimitsFeatures = { readonly [F in Feature]: (typeof LIMITS_IMPLEMENTATIONS)[F][number] | null };

type FeatureRow = { readonly [F in Feature]: string | null };

function named<T extends string>(known: readonly T[], value: string | null): T | null {
  return known.find((name) => name === value) ?? null;
}

function features(row: FeatureRow): LimitsFeatures {
  return {
    signInDeadline: named(LIMITS_IMPLEMENTATIONS.signInDeadline, row.signInDeadline),
    subscriptionTerm: named(LIMITS_IMPLEMENTATIONS.subscriptionTerm, row.subscriptionTerm),
    bankedResets: named(LIMITS_IMPLEMENTATIONS.bankedResets, row.bankedResets),
    paidResetOffer: named(LIMITS_IMPLEMENTATIONS.paidResetOffer, row.paidResetOffer),
  };
}

export const LIMITS_FEATURES = Object.fromEntries(
  LIMITS_PROVIDERS.map((provider) => [provider, features(LIMITS_FEATURE_ROWS[provider])]),
) as Readonly<Record<LimitsProvider, LimitsFeatures>>;
