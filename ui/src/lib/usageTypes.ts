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
