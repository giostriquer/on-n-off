import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { clearUsageHistory, usageHistoryStatus } from "./api";

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue({ state: "empty", bytes: 0 });
});

it("reads and clears the usage history through the commands lib.rs registers", async () => {
  await usageHistoryStatus();
  await clearUsageHistory();

  expect(invoke.mock.calls).toEqual([["usage_history_status"], ["clear_usage_history"]]);
});
