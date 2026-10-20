import { LIMITS_FEATURE_ROWS, type LimitsProvider } from "$lib/providers";

export type LimitsFeatures = {
  signInDeadline: "claude" | null;
  subscriptionTerm: "codex" | null;
  bankedResets: "codex" | null;
  paidResetOffer: "codex" | null;
};

export const LIMITS_FEATURES = LIMITS_FEATURE_ROWS as Readonly<Record<LimitsProvider, LimitsFeatures>>;
