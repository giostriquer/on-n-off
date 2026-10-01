import type { AgentId } from "./types";

export type LimitsStatus = "ok" | "signedOut" | "unauthenticated" | "unsupported" | "failed";
export type LimitWindowKind = "session" | "weekly" | "model";

export type LimitWindow = {
  id: string;
  label: string;
  kind: LimitWindowKind;
  usedPercent: number;
  resetsAt?: string | null;
  windowSeconds?: number | null;
  observedAt: string;
};

export type LimitsCredits = {
  balance: string;
  unlimited: boolean;
};

export type LimitsWorkspaceCredits = {
  limit: string;
  used: string;
  usedPercent: number;
  resetsAt?: string | null;
  reached: boolean;
};

export type LimitsCreditsSpent = {
  last7Days: number;
  last30Days: number;
  updatedAt?: string | null;
};

export type LimitsSubscription = {
  activeUntil: string;
  willRenew: boolean;
  note?: "cancelled" | "planChange" | "pastDue" | null;
  checkedAt: string;
};

export type LimitsResetCredits = {
  availableCount: number;
  nextExpiresAt?: string | null;
  resets?: LimitsBankedReset[];
};

export type LimitsBankedReset = {
  title?: string | null;
  expiresAt?: string | null;
};

export type ResetCreditOutcome = "reset" | "nothingToReset" | "noCredit" | "alreadyRedeemed" | "unknown";

export type LimitsAccount = {
  id: string;
  label?: string | null;
};

export type LimitsPrice = {
  amountMinorUnits: number;
  currency: string;
};

export type LimitsResetOffer = {
  price?: LimitsPrice | null;
};

export type ProviderLimits = {
  provider: AgentId;
  status: LimitsStatus;
  message?: string | null;
  account?: LimitsAccount | null;
  currentAccount: boolean;
  savedProfile?: boolean;
  archived?: boolean;
  plan?: string | null;
  windows: LimitWindow[];
  credits?: LimitsCredits | null;
  workspaceCredits?: LimitsWorkspaceCredits | null;
  creditsSpent?: LimitsCreditsSpent | null;
  subscription?: LimitsSubscription | null;
  resetCredits?: LimitsResetCredits | null;
  resetOffer?: LimitsResetOffer | null;
};
