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

/** The subscription account a snapshot belongs to; `label` is the email when the CLI stores one. */
export type LimitsAccount = {
  id: string;
  label?: string | null;
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
  windows: LimitWindow[];
  credits?: LimitsCredits | null;
};
