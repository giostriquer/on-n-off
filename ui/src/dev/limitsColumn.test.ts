import { expect, it } from "vitest";
import type { LimitsStatus, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import cases from "../../../src-tauri/src/limits/column/fixtures/supersedes-cases.json";
import { supersedes } from "./limitsColumn";

type Side = { provider: string; status: string; current: boolean; id: string; legacyId: string | null; label: string | null; observed: boolean };

const WEEKLY = { id: "weekly", label: "Weekly · all models", kind: "weekly" as const, usedPercent: 40, observedAt: "2026-08-24T20:00:00Z" };

function card(side: Side): ProviderLimits {
  return {
    provider: side.provider as AgentId, status: side.status as LimitsStatus, currentAccount: side.current,
    account: { id: side.id, legacyId: side.legacyId, label: side.label }, windows: side.observed ? [WEEKLY] : [],
  };
}

it.each(cases.map(shared => [shared.case, shared] as const))("folds as the Rust rule does: %s", (_case, shared) => {
  expect(supersedes(card(shared.scoped), card(shared.legacy))).toBe(shared.supersedes);
});
