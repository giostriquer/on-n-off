import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { cancelResetSpend, clearUsageHistory, pendingResetSpends, setLimitsArchived, usageHistoryStatus } from "./api";

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue({ state: "empty", bytes: 0 });
});

it("reads and clears the usage history through the commands lib.rs registers", async () => {
  await usageHistoryStatus();
  await clearUsageHistory();

  expect(invoke.mock.calls).toEqual([["usage_history_status"], ["clear_usage_history"]]);
});

it("lists and cancels waiting automatic resets through the commands lib.rs registers", async () => {
  await pendingResetSpends();
  await cancelResetSpend("acct-work");

  expect(invoke.mock.calls).toEqual([["pending_reset_spends"], ["cancel_reset_spend", { accountId: "acct-work" }]]);
});

it("archives the accounts a card names through the command lib.rs registers", async () => {
  await setLimitsArchived("codex", ["team"], true);

  expect(invoke.mock.calls).toEqual([["set_limits_archived", { agentId: "codex", accountIds: ["team"], archived: true }]]);
});
