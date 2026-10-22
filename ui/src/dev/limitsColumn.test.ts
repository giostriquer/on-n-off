import { expect, it } from "vitest";
import type { LimitsStatus, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import cases from "../../../src-tauri/src/limits/column/fixtures/supersedes-cases.json";
import { supersedes } from "./limitsColumn";

type Side = { provider: string; status: string; current: boolean; id: string; legacyId: string | null; label: string | null; observed: string };

const WEEKLY = { id: "weekly", label: "Weekly · all models", kind: "weekly" as const, usedPercent: 40, observedAt: "2026-08-24T20:00:00Z" };

function reading(observed: string): Partial<ProviderLimits> {
  switch (observed) {
    case "windows": return { windows: [WEEKLY] };
    case "credits": return { credits: { balance: "5", unlimited: false } };
    case "workspaceCredits": return { workspaceCredits: { limit: "100", used: "40", usedPercent: 40, reached: false } };
    case "creditsSpent": return { creditsSpent: { last7Days: 1, last30Days: 2 } };
    case "bankedResets": return { resetCredits: { availableCount: 1 } };
    case "noBankedResets": return { resetCredits: { availableCount: 0 } };
    case "nothing": return {};
    default: throw new Error(`unknown observation ${observed}`);
  }
}

function card(side: Side): ProviderLimits {
  return {
    provider: side.provider as AgentId, status: side.status as LimitsStatus, currentAccount: side.current,
    account: { id: side.id, legacyId: side.legacyId, label: side.label }, windows: [], ...reading(side.observed),
  };
}

it.each(cases.map(shared => [shared.case, shared] as const))("folds as the Rust rule does: %s", (_case, shared) => {
  expect(supersedes(card(shared.scoped), card(shared.legacy))).toBe(shared.supersedes);
});
