import type { AgentId } from "./types";

export type LimitsStatus = "ok" | "signedOut" | "unauthenticated" | "unsupported" | "failed";
export type LimitWindowKind = "session" | "weekly" | "model";

export type LimitWindow = {
  id: string;
  label: string;
  kind: LimitWindowKind;
  /** 0..=100 */
  usedPercent: number;
  /** RFC 3339 instant, when the provider reports one. */
  resetsAt?: string | null;
  /** Canonical window duration when the source reports one. */
  windowSeconds?: number | null;
  /** RFC 3339 instant when this window's percentage was observed. */
  observedAt: string;
};

export type LimitsCredits = {
  balance: string;
  unlimited: boolean;
};

/**
 * A business workspace member's share of the workspace's pooled credits (Codex's spend control):
 * how many of them this member may use, how many are used, and when the share resets.
 */
export type LimitsWorkspaceCredits = {
  /** Amounts as the provider states them: finite numbers of at least zero, which may carry decimals. */
  limit: string;
  used: string;
  /** How much of the share is used, 0–100: Codex's own meter, worked out once by the reader. */
  usedPercent: number;
  resetsAt?: string | null;
  reached: boolean;
};

/**
 * What a business workspace member spent lately, counted the way the Codex app's "Credit usage
 * history" counts it: the last 7 and 30 UTC days of per-model credits. There is no limit beside it.
 */
export type LimitsCreditsSpent = {
  last7Days: number;
  last30Days: number;
  /** When the provider's usage data runs up to; it can trail the read by hours. */
  updatedAt?: string | null;
};

/** Codex banked rate-limit resets: one-time resets saved to the account until used or expired. */
export type LimitsResetCredits = {
  availableCount: number;
  /** RFC 3339 instant the soonest-expiring available reset lapses, when Codex reports it. */
  nextExpiresAt?: string | null;
};

/**
 * What Codex did with a request to spend one banked reset. `unknown`: Codex answered with an outcome
 * this build does not recognise; the request still went through.
 */
export type ResetCreditOutcome = "reset" | "nothingToReset" | "noCredit" | "alreadyRedeemed" | "unknown";

/** The subscription account a snapshot belongs to; `label` is the email when the CLI stores one. */
export type LimitsAccount = {
  id: string;
  label?: string | null;
};

/** A price as the provider states it: minor units and the currency they count. */
export type LimitsPrice = {
  amountMinorUnits: number;
  currency: string;
};

/** A paid reset the provider is offering right now. It may name no price. */
export type LimitsResetOffer = {
  price?: LimitsPrice | null;
};

/**
 * Mirrors `ProviderLimitsDto`: provider-side problems arrive as a status, not an error.
 * `currentAccount: false` is an account remembered independently of the CLI's current login.
 */
export type ProviderLimits = {
  provider: AgentId;
  status: LimitsStatus;
  message?: string | null;
  account?: LimitsAccount | null;
  currentAccount: boolean;
  plan?: string | null;
  /** Claude only: the profile's `organization.subscription_status`, as Anthropic writes it. */
  subscriptionStatus?: string | null;
  windows: LimitWindow[];
  credits?: LimitsCredits | null;
  workspaceCredits?: LimitsWorkspaceCredits | null;
  creditsSpent?: LimitsCreditsSpent | null;
  resetCredits?: LimitsResetCredits | null;
  /** A paid reset offered right now. Absent whenever the account is not at its limit. */
  resetOffer?: LimitsResetOffer | null;
};
