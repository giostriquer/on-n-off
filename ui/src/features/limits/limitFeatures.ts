import type { LimitsProvider } from "$lib/providers";

export type LimitsFeatures = {
  subscriptionTerm: "codex" | null;
  firstMessageSession: "claude" | null;
  bankedResets: "codex" | null;
  paidResetOffer: "codex" | null;
};

export const LIMITS_FEATURES: Readonly<Record<LimitsProvider, LimitsFeatures>> = {
  claude: { subscriptionTerm: null, firstMessageSession: "claude", bankedResets: null, paidResetOffer: null },
  codex: { subscriptionTerm: "codex", firstMessageSession: null, bankedResets: "codex", paidResetOffer: "codex" },
  muse: { subscriptionTerm: null, firstMessageSession: "claude", bankedResets: null, paidResetOffer: null },
};
