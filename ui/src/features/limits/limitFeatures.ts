import type { LimitsProvider } from "$lib/providers";

export type LimitsFeatures = {
  signInDeadline: "claude" | null;
  subscriptionTerm: "codex" | null;
  firstMessageSession: "claude" | null;
  bankedResets: "codex" | null;
  paidResetOffer: "codex" | null;
};

export const LIMITS_FEATURES: Readonly<Record<LimitsProvider, LimitsFeatures>> = {
  claude: { signInDeadline: "claude", subscriptionTerm: null, firstMessageSession: "claude", bankedResets: null, paidResetOffer: null },
  codex: { signInDeadline: null, subscriptionTerm: "codex", firstMessageSession: null, bankedResets: "codex", paidResetOffer: "codex" },
  muse: { signInDeadline: null, subscriptionTerm: null, firstMessageSession: "claude", bankedResets: null, paidResetOffer: null },
};
