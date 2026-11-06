import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const open = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));
const listen = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/event", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/event")>()),
  listen,
}));

import { cancelResetSpend, clearUsageHistory, installPlugin, onMainWindowFocused, pendingResetSpends, pickCliBinary, pickFolder, removeLimitsAccount, setLimitsAccountArchived, setLimitsArchived, setMcpEnabled, setPluginEnabled, setSkillEnabled, uninstallPlugin, updatePlugin, usageHistoryStatus } from "./api";

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

it("removes, archives and unarchives a card by its account alone, one command each", async () => {
  await removeLimitsAccount("codex", "profile:team");
  await setLimitsAccountArchived("claude", "user-a", true);
  await setLimitsAccountArchived("muse", "email:you@example.com", false);

  expect(invoke.mock.calls).toEqual([
    ["remove_limits_account", { agentId: "codex", accountId: "profile:team" }],
    ["set_limits_account_archived", { agentId: "claude", accountId: "user-a", archived: true }],
    ["set_limits_account_archived", { agentId: "muse", accountId: "email:you@example.com", archived: false }],
  ]);
});

it("hears only the main window's own focus event, the one Tauri emits when that window gains focus", async () => {
  const stop = vi.fn();
  listen.mockResolvedValueOnce(stop);
  const handler = vi.fn();

  expect(await onMainWindowFocused(handler)).toBe(stop);
  expect(listen).toHaveBeenCalledWith("tauri://focus", expect.any(Function), { target: { kind: "Window", label: "main" } });

  listen.mock.calls[0]![1]({ event: "tauri://focus", id: 1, payload: null });
  expect(handler).toHaveBeenCalledTimes(1);
});

it.each([
  {
    command: "set_plugin_enabled",
    send: (scope?: string | null) => setPluginEnabled("codex", "workbench@workshop", false, scope),
    args: { agentId: "codex", pluginId: "workbench@workshop", enabled: false },
  },
  {
    command: "set_skill_enabled",
    send: (scope?: string | null) => setSkillEnabled("codex", "acme-notes", true, scope),
    args: { agentId: "codex", skillId: "acme-notes", enabled: true },
  },
  {
    command: "set_mcp_enabled",
    send: (scope?: string | null) => setMcpEnabled("codex", "github", false, scope),
    args: { agentId: "codex", mcpId: "github", enabled: false },
  },
  {
    command: "install_plugin",
    send: (scope?: string | null) => installPlugin("codex", "acme@webapp", scope),
    args: { agentId: "codex", source: "acme@webapp" },
  },
  {
    command: "uninstall_plugin",
    send: (scope?: string | null) => uninstallPlugin("codex", "workbench@workshop", scope),
    args: { agentId: "codex", pluginId: "workbench@workshop" },
  },
  {
    command: "update_plugin",
    send: (scope?: string | null) => updatePlugin("codex", "workbench@workshop", scope),
    args: { agentId: "codex", pluginId: "workbench@workshop" },
  },
])("$command sends the scope in view, and null for the global view", async ({ command, send, args }) => {
  await send("/Users/me/webapp");
  await send(null);
  await send();

  expect(invoke.mock.calls).toEqual([
    [command, { ...args, projectPath: "/Users/me/webapp" }],
    [command, { ...args, projectPath: null }],
    [command, { ...args, projectPath: null }],
  ]);
});
