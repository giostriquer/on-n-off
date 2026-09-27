import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { clearUsageHistory, setLimitsArchived, usageHistoryStatus } from "./api";

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue({ state: "empty", bytes: 0 });
});

it("reads and clears the usage history through the commands lib.rs registers", async () => {
  await usageHistoryStatus();
  await clearUsageHistory();

  expect(invoke.mock.calls).toEqual([["usage_history_status"], ["clear_usage_history"]]);
});

it("archives the accounts a card names through the command lib.rs registers", async () => {
  await setLimitsArchived("codex", ["team"], true);

  expect(invoke.mock.calls).toEqual([["set_limits_archived", { agentId: "codex", accountIds: ["team"], archived: true }]]);
});
