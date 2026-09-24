import type { AgentId } from "./types";

export type UsageCostSource = "providerReported" | "modelPriced" | "unpriced";
export type UsageSourceStatus = "ok" | "missing" | "partial" | "failed";
export type UsagePricingStatus = "fresh" | "cached" | "unavailable";
export type UsageResolution = "day" | "hour";
export type UsageMetric = "cost" | "tokens";

export type UsageTokenTotals = {
  uncachedInputTokens: number;
  cachedInputTokens: number;
  cacheCreationTokens: number;
  outputTokens: number;
  reasoningTokens: number;
};

export type UsageBucket = {
  day: string;
  hourStart?: string;
  provider: AgentId;
  model: string;
  totals: UsageTokenTotals;
  costUsd: number;
  cacheSavingsUsd: number;
  costSource: UsageCostSource;
  records: number;
  unpricedRecords: number;
  sessions: number;
};

export type UsageSource = {
  provider: AgentId;
  status: UsageSourceStatus;
  scannedFiles: number;
  skippedFiles: number;
  malformedRecords: number;
  distinctSessions: number;
  message?: string | null;
  resolvedPath: string;
};

export type UsagePricing = {
  status: UsagePricingStatus;
  source: string;
  fetchedAt?: string | null;
  knownModels: number;
};

export type UsageSummary = {
  readAt: string;
  timeZone: string;
  sinceDay: string;
  untilDay: string;
  buckets: UsageBucket[];
  sources: UsageSource[];
  pricing: UsagePricing;
  scanDurationMs: number;
  cacheHit?: boolean;
};

export type UsageSummaryInput = {
  sinceDay: string;
  untilDay: string;
  timeZone: string;
  resolution?: UsageResolution;
  sinceTime?: string;
  untilTime?: string;
  force?: boolean;
};

/** What the usage kept after transcripts are deleted holds (`usage/history.rs`). */
export type UsageHistoryState = "empty" | "kept" | "unreadable";

export type UsageHistoryStatus = {
  state: UsageHistoryState;
  /** Start of the oldest usage kept (RFC 3339). */
  keptSince?: string | null;
  /** The history counts usage before this instant; transcripts count from it on. */
  foldedThrough?: string | null;
  bytes: number;
};
