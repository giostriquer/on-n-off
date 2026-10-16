import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const open = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));

import { cancelResetSpend, clearUsageHistory, pendingResetSpends, pickCliBinary, pickFolder, setLimitsArchived, usageHistoryStatus } from "./api";

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue({ state: "empty", bytes: 0 });
  open.mockReset();
});

it("picks one folder, or a CLI launcher, through the dialog plugin and answers null for anything else", async () => {
  open.mockResolvedValueOnce("/Users/me/webapp").mockResolvedValueOnce(null);
  expect(await pickFolder()).toBe("/Users/me/webapp");
  expect(await pickFolder()).toBeNull();
  open.mockResolvedValueOnce("/opt/acme/bin/codex.cmd").mockResolvedValueOnce(["/opt/a", "/opt/b"]);
  expect(await pickCliBinary()).toBe("/opt/acme/bin/codex.cmd");
  expect(await pickCliBinary()).toBeNull();

  const folder = { directory: true, multiple: false };
  const cli = { multiple: false, filters: [{ name: "CLI", extensions: ["exe", "cmd", "bat"] }] };
  expect(open.mock.calls).toEqual([[folder], [folder], [cli], [cli]]);
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
