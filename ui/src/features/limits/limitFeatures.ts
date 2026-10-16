import type { LimitsProvider } from "$lib/providers";

export type LimitsFeatures = {
  signInDeadline: boolean;
  subscriptionTerm: boolean;
  paidResetOffer: boolean;
  firstMessageSession: boolean;
  bankedResetActions: boolean;
};

export const LIMITS_FEATURES: Readonly<Record<LimitsProvider, LimitsFeatures>> = {
  claude: { signInDeadline: true, subscriptionTerm: false, paidResetOffer: false, firstMessageSession: true, bankedResetActions: false },
  codex: { signInDeadline: false, subscriptionTerm: true, paidResetOffer: true, firstMessageSession: false, bankedResetActions: true },
  muse: { signInDeadline: false, subscriptionTerm: false, paidResetOffer: false, firstMessageSession: false, bankedResetActions: false },
};
