/**
 * Fold one machine's UsageSummary buckets into page totals.
 * Multi-environment merge is out of scope for on-n-off.
 */

import type { AgentId } from "./types";
import type { UsageBucket, UsageSummary } from "./usageTypes";

export type ProviderTotals = {
  provider: AgentId;
  costUsd: number;
  totalTokens: number;
  records: number;
  costShare: number;
  tokenShare: number;
};

export type ModelTotals = {
  model: string;
  provider: AgentId;
  costUsd: number;
  totalTokens: number;
  records: number;
  costShare: number;
  tokenShare: number;
};

export type PeriodProviderSlice = {
  costUsd: number;
  totalTokens: number;
};

export type PeriodTotals = {
  day: string;
  hourStart?: string;
  costUsd: number;
  totalTokens: number;
  byProvider: Readonly<Record<AgentId, PeriodProviderSlice>>;
};

export type TokenBreakdown = {
  uncachedInputTokens: number;
  cachedInputTokens: number;
  cacheCreationTokens: number;
  outputTokens: number;
  reasoningTokens: number;
};

export type FoldedUsage = {
  costUsd: number;
  totalTokens: number;
  records: number;
  sessions: number;
  cacheSavingsUsd: number;
  activeDays: number;
  tokens: TokenBreakdown;
  providers: readonly ProviderTotals[];
  models: readonly ModelTotals[];
  daily: readonly PeriodTotals[];
  hourly: readonly PeriodTotals[];
};

const PROVIDERS: AgentId[] = ["claude", "codex"];

function emptyByProvider(): Record<AgentId, PeriodProviderSlice> {
  return {
    claude: { costUsd: 0, totalTokens: 0 },
    codex: { costUsd: 0, totalTokens: 0 },
    antigravity: { costUsd: 0, totalTokens: 0 },
  };
}

function bucketTokens(bucket: UsageBucket): number {
  const t = bucket.totals;
  return t.uncachedInputTokens + t.cachedInputTokens + t.cacheCreationTokens + t.outputTokens;
}

const EMPTY_TOKENS: TokenBreakdown = {
  uncachedInputTokens: 0,
  cachedInputTokens: 0,
  cacheCreationTokens: 0,
  outputTokens: 0,
  reasoningTokens: 0,
};

const EMPTY: FoldedUsage = {
  costUsd: 0,
  totalTokens: 0,
  records: 0,
  sessions: 0,
  cacheSavingsUsd: 0,
  activeDays: 0,
  tokens: EMPTY_TOKENS,
  providers: [],
  models: [],
  daily: [],
  hourly: [],
};

export function foldUsage(summary: UsageSummary | null): FoldedUsage {
  if (!summary || summary.buckets.length === 0) {
    return {
      ...EMPTY,
      sessions: summary?.sources.reduce((n, s) => n + s.distinctSessions, 0) ?? 0,
    };
  }

  let costUsd = 0;
  let totalTokens = 0;
  let records = 0;
  let cacheSavingsUsd = 0;
  const tokenAcc: TokenBreakdown = { ...EMPTY_TOKENS };
  const providerAcc = new Map<AgentId, { costUsd: number; totalTokens: number; records: number }>();
  const modelAcc = new Map<string, { provider: AgentId; costUsd: number; totalTokens: number; records: number }>();
  const dailyAcc = new Map<string, { costUsd: number; totalTokens: number; byProvider: Record<AgentId, PeriodProviderSlice> }>();
  const hourlyAcc = new Map<
    string,
    { day: string; costUsd: number; totalTokens: number; byProvider: Record<AgentId, PeriodProviderSlice> }
  >();

  for (const bucket of summary.buckets) {
    const tokens = bucketTokens(bucket);
    costUsd += bucket.costUsd;
    totalTokens += tokens;
    records += bucket.records;
    cacheSavingsUsd += bucket.cacheSavingsUsd;
    tokenAcc.uncachedInputTokens += bucket.totals.uncachedInputTokens;
    tokenAcc.cachedInputTokens += bucket.totals.cachedInputTokens;
    tokenAcc.cacheCreationTokens += bucket.totals.cacheCreationTokens;
    tokenAcc.outputTokens += bucket.totals.outputTokens;
    tokenAcc.reasoningTokens += bucket.totals.reasoningTokens;

    const provider = providerAcc.get(bucket.provider) ?? { costUsd: 0, totalTokens: 0, records: 0 };
    provider.costUsd += bucket.costUsd;
    provider.totalTokens += tokens;
    provider.records += bucket.records;
    providerAcc.set(bucket.provider, provider);

    const modelKey = `${bucket.provider}\0${bucket.model}`;
    const model = modelAcc.get(modelKey) ?? {
      provider: bucket.provider,
      costUsd: 0,
      totalTokens: 0,
      records: 0,
    };
    model.costUsd += bucket.costUsd;
    model.totalTokens += tokens;
    model.records += bucket.records;
    modelAcc.set(modelKey, model);

    const day = dailyAcc.get(bucket.day) ?? {
      costUsd: 0,
      totalTokens: 0,
      byProvider: emptyByProvider(),
    };
    day.costUsd += bucket.costUsd;
    day.totalTokens += tokens;
    day.byProvider[bucket.provider].costUsd += bucket.costUsd;
    day.byProvider[bucket.provider].totalTokens += tokens;
    dailyAcc.set(bucket.day, day);

    if (bucket.hourStart) {
      const hour = hourlyAcc.get(bucket.hourStart) ?? {
        day: bucket.day,
        costUsd: 0,
        totalTokens: 0,
        byProvider: emptyByProvider(),
      };
      hour.costUsd += bucket.costUsd;
      hour.totalTokens += tokens;
      hour.byProvider[bucket.provider].costUsd += bucket.costUsd;
      hour.byProvider[bucket.provider].totalTokens += tokens;
      hourlyAcc.set(bucket.hourStart, hour);
    }
  }

  const sessions = summary.sources.reduce((n, s) => n + s.distinctSessions, 0);

  const providers: ProviderTotals[] = [...providerAcc.entries()]
    .map(([provider, totals]) => ({
      provider,
      costUsd: totals.costUsd,
      totalTokens: totals.totalTokens,
      records: totals.records,
      costShare: costUsd === 0 ? 0 : totals.costUsd / costUsd,
      tokenShare: totalTokens === 0 ? 0 : totals.totalTokens / totalTokens,
    }))
    .sort((a, b) => b.costUsd - a.costUsd || b.totalTokens - a.totalTokens);

  const models: ModelTotals[] = [...modelAcc.entries()]
    .map(([key, totals]) => ({
      model: key.split("\0")[1] ?? "",
      provider: totals.provider,
      costUsd: totals.costUsd,
      totalTokens: totals.totalTokens,
      records: totals.records,
      costShare: costUsd === 0 ? 0 : totals.costUsd / costUsd,
      tokenShare: totalTokens === 0 ? 0 : totals.totalTokens / totalTokens,
    }))
    .sort((a, b) => b.costUsd - a.costUsd || b.totalTokens - a.totalTokens);

  const daily: PeriodTotals[] = [...dailyAcc.entries()]
    .map(([day, totals]) => ({ day, ...totals }))
    .sort((a, b) => a.day.localeCompare(b.day));

  const hourly: PeriodTotals[] = [...hourlyAcc.entries()]
    .map(([hourStart, totals]) => ({
      day: totals.day,
      hourStart,
      costUsd: totals.costUsd,
      totalTokens: totals.totalTokens,
      byProvider: totals.byProvider,
    }))
    .sort((a, b) => (a.hourStart ?? "").localeCompare(b.hourStart ?? ""));

  const activeDays = daily.reduce((n, period) => n + (period.costUsd > 0 || period.totalTokens > 0 ? 1 : 0), 0);

  return {
    costUsd,
    totalTokens,
    records,
    sessions,
    cacheSavingsUsd,
    activeDays,
    tokens: tokenAcc,
    providers,
    models,
    daily,
    hourly,
  };
}

export function providerLabel(provider: AgentId): string {
  switch (provider) {
    case "claude":
      return "Claude";
    case "codex":
      return "Codex";
    case "antigravity":
      return "Antigravity";
  }
}

export { PROVIDERS };
