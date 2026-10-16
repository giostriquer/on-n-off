import { defaultNotchSettings } from "$lib/notchTypes";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { Settings } from "./Settings";
import { UpdateProvider } from "@/features/updater/UpdateProvider";
import type { AgentInfo, AppSettings } from "$lib/types";
import type { AppSettingsChange } from "$lib/appSettingsDocument";
import type { UpdaterClient } from "@/features/updater/updaterClient";

const apiMocks = vi.hoisted(() => ({
  requestNotificationPermission: vi.fn<() => Promise<boolean>>(),
}));

vi.mock("$lib/api", () => ({
  readAccountPreferences: vi.fn().mockResolvedValue(false),
  accountAction: vi.fn().mockResolvedValue(undefined),
  onSharedReadChanged: vi.fn().mockResolvedValue(() => undefined),
  readNotchState: () => Promise.resolve({ revision: 0, supported: false, settings: defaultNotchSettings(), displays: [], error: null }),
  onNotchChanged: () => Promise.resolve(() => undefined),
  saveNotchSettings: vi.fn(),
  usageHistoryStatus: () => Promise.resolve({ state: "empty", bytes: 0 }),
  diagnoseProviders: () =>
    Promise.resolve([
      {
        agentId: "claude",
        binary: "claude",
        homePath: "C:\\Users\\me\\.claude",
        checks: [
          {
            id: "cli",
            label: "CLI binary",
            ok: false,
            detail: "claude is not on PATH",
            hint: "Set a binary path.",
          },
        ],
      },
    ]),
  requestNotificationPermission: apiMocks.requestNotificationPermission,
  traySupported: () => Promise.resolve(true),
}));

const agents: AgentInfo[] = [
  {
    id: "claude",
    displayName: "Claude",
    cliOk: false,
    cliError: "Claude CLI not found.",
    installGit: false,
    installFolder: false,
    pluginToggle: false,
    readsHooks: true,
  },
  {
    id: "codex",
    displayName: "Codex",
    cliOk: true,
    cliError: null,
    installGit: true,
    installFolder: true,
    pluginToggle: true,
    readsHooks: true,
  },
  {
    id: "antigravity",
    displayName: "Antigravity",
    cliOk: false,
    cliError: null,
    installGit: false,
    installFolder: false,
    pluginToggle: false,
    readsHooks: false,
  },
  {
    id: "cursor",
    displayName: "Cursor",
    cliOk: false,
    cliError: null,
    installGit: false,
    installFolder: false,
    pluginToggle: false,
    readsHooks: false,
  },
];

const updaterClient: UpdaterClient = {
  buildInfo: async () => ({ enabled: false, installerKind: null, target: null }),
  currentVersion: async () => "0.1.0",
  check: async () => null,
  relaunch: async () => undefined,
};

function renderSettings({
  onToggleVisible = () => undefined,
  onSettingsChange = () => undefined,
  githubScopes = [],
  resetAlerts = {},
}: {
  onToggleVisible?: (id: AgentInfo["id"], hidden: boolean) => void;
  onSettingsChange?: (change: AppSettingsChange) => void;
  githubScopes?: string[];
  resetAlerts?: AppSettings["resetAlerts"];
} = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <UpdateProvider initialProviderReady automaticUpdates client={updaterClient}>
        <Settings
          agents={agents}
          settings={settingsWith({ githubScopes, resetAlerts })}
          onToggleVisible={onToggleVisible}
          onSaveBinary={() => undefined}
          onSettingsChange={onSettingsChange}
        />
      </UpdateProvider>
    </QueryClientProvider>,
  );
}

function settingsWith(fields: Pick<AppSettings, "githubScopes" | "resetAlerts">): AppSettings {
  return {
    hiddenAgents: [],
    binaryPaths: {},
    automaticUpdates: true,
    limitNotifications: false,
    limitsPollMinutes: 10,
    githubNotifications: false,
    githubPollSeconds: 60,
    closeToTray: false,
    ...fields,
  };
}

function lastChangeTo(onSettingsChange: Mock<(change: AppSettingsChange) => void>, latest: AppSettings) {
  const change = onSettingsChange.mock.lastCall![0];
  return typeof change === "function" ? change(latest) : change;
}

describe("Settings", () => {
  beforeEach(() => {
    apiMocks.requestNotificationPermission.mockReset();
    apiMocks.requestNotificationPermission.mockResolvedValue(true);
  });

  it("renders provider cards and can hide one from tabs", async () => {
    const user = userEvent.setup();
    const onToggleVisible = vi.fn();
    renderSettings({ onToggleVisible });
    expect(screen.getAllByText("Claude").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Codex").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /Show Claude in agent tabs/i })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Show Antigravity in agent tabs/i }));
    expect(onToggleVisible).toHaveBeenCalledWith("antigravity", true);
  });

  it("shows how far back usage is kept", async () => {
    renderSettings();

    const card = await screen.findByRole("region", { name: "Usage history" });
    await waitFor(() => expect(card).toHaveTextContent("Nothing kept yet"));
  });

  it("uses Cursor's `agent` command as the binary placeholder", () => {
    renderSettings();

    expect(screen.getByPlaceholderText("agent")).toBeTruthy();
    expect(screen.queryByPlaceholderText("cursor-agent")).toBeNull();
  });

  it("requests notification permission before enabling limit polling and persists the interval", async () => {
    const user = userEvent.setup();
    const onSettingsChange = vi.fn();
    renderSettings({ onSettingsChange });

    await user.click(screen.getByRole("button", { name: "Notify about limit changes" }));

    expect(apiMocks.requestNotificationPermission).toHaveBeenCalledTimes(1);
    expect(onSettingsChange).toHaveBeenCalledWith({ limitNotifications: true });

    await user.selectOptions(
      screen.getByRole("combobox", { name: "Limits polling interval" }),
      "15",
    );
    expect(onSettingsChange).toHaveBeenCalledWith({ limitsPollMinutes: 15 });
  });

  it("turns automatic updates off through the shared settings patch", async () => {
    const user = userEvent.setup();
    const onSettingsChange = vi.fn();
    renderSettings({ onSettingsChange });

    await user.click(screen.getByRole("button", { name: "Automatically download updates" }));

    expect(onSettingsChange).toHaveBeenCalledWith({ automaticUpdates: false });
  });

  it("keeps limit polling disabled when notification permission is denied", async () => {
    apiMocks.requestNotificationPermission.mockResolvedValue(false);
    const user = userEvent.setup();
    const onSettingsChange = vi.fn();
    renderSettings({ onSettingsChange });

    await user.click(screen.getByRole("button", { name: "Notify about limit changes" }));

    expect(onSettingsChange).not.toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Notifications are blocked in system settings.",
    );
  });

  it("adds and removes GitHub scopes with Enter and the chip button, never on blur", async () => {
    const user = userEvent.setup();
    const onSettingsChange = vi.fn<(change: AppSettingsChange) => void>();
    const rendered = settingsWith({ githubScopes: ["org:acme", "repo:me/tool"], resetAlerts: {} });
    renderSettings({ onSettingsChange, githubScopes: rendered.githubScopes });

    const card = screen.getByRole("region", { name: "Pull requests" });
    expect(card).toHaveTextContent("org:acme");
    expect(card).toHaveTextContent("repo:me/tool");

    const input = screen.getByRole("textbox", { name: "Scopes" });
    expect(input).toHaveAccessibleDescription(/org:NAME, user:NAME or OWNER\/REPO/);
    await user.type(input, "  user:me{Enter}");
    expect(onSettingsChange).toHaveBeenCalledTimes(1);
    expect(lastChangeTo(onSettingsChange, rendered)).toEqual({ githubScopes: ["org:acme", "repo:me/tool", "user:me"] });
    expect(lastChangeTo(onSettingsChange, { ...rendered, githubScopes: ["org:other"] })).toEqual({
      githubScopes: ["org:other", "user:me"],
    });
    expect(lastChangeTo(onSettingsChange, { ...rendered, githubScopes: ["user:me"] })).toEqual({ githubScopes: ["user:me"] });
    expect(input).toHaveValue("");

    await user.click(screen.getByRole("button", { name: "Remove org:acme" }));
    expect(onSettingsChange).toHaveBeenCalledTimes(2);
    expect(lastChangeTo(onSettingsChange, rendered)).toEqual({ githubScopes: ["repo:me/tool"] });
    expect(lastChangeTo(onSettingsChange, { ...rendered, githubScopes: ["org:acme", "org:other"] })).toEqual({
      githubScopes: ["org:other"],
    });

    onSettingsChange.mockClear();
    await user.type(input, "   {Enter}");
    expect(onSettingsChange).not.toHaveBeenCalled();

    await user.type(input, "org:acme{Enter}");
    expect(onSettingsChange).not.toHaveBeenCalled();
    expect(input).toHaveValue("");

    await user.type(input, "org:other");
    await user.tab();
    expect(onSettingsChange).not.toHaveBeenCalled();
    expect(input).toHaveValue("org:other");
  });

  it("requests notification permission before enabling pull request notifications and persists the interval", async () => {
    const user = userEvent.setup();
    const onSettingsChange = vi.fn();
    renderSettings({ onSettingsChange });

    await user.click(screen.getByRole("button", { name: "Notify about CI, review and merge changes" }));

    expect(apiMocks.requestNotificationPermission).toHaveBeenCalledTimes(1);
    expect(onSettingsChange).toHaveBeenCalledWith({ githubNotifications: true });

    await user.selectOptions(screen.getByRole("combobox", { name: "GitHub polling interval" }), "120");
    expect(onSettingsChange).toHaveBeenCalledWith({ githubPollSeconds: 120 });
  });

  it("keeps pull request notifications disabled when notification permission is denied", async () => {
    apiMocks.requestNotificationPermission.mockResolvedValue(false);
    const user = userEvent.setup();
    const onSettingsChange = vi.fn();
    renderSettings({ onSettingsChange });

    await user.click(screen.getByRole("button", { name: "Notify about CI, review and merge changes" }));

    expect(onSettingsChange).not.toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent("Notifications are blocked in system settings.");
  });

  it("routes the tray card through the shared settings patch", async () => {
    const user = userEvent.setup();
    const onSettingsChange = vi.fn();
    renderSettings({ onSettingsChange });

    const card = await screen.findByRole("region", { name: "Windows tray" });
    expect(card).toHaveTextContent(/notification area/i);

    await user.click(
      screen.getByRole("button", {
        name: "Keep on-n-off running in the tray when the window is closed",
      }),
    );
    expect(onSettingsChange).toHaveBeenCalledWith({ closeToTray: true });
  });
  it("lists each banked reset alert and turns one off", async () => {
    const user = userEvent.setup();
    const onSettingsChange = vi.fn<(change: AppSettingsChange) => void>();
    const side = { label: null, maxLeftPercent: 3, minHoursToRenewal: 48, automatic: true };
    const rendered = settingsWith({
      githubScopes: [],
      resetAlerts: {
        "acct-work": { label: "work@example.com", maxLeftPercent: 5, minHoursToRenewal: 24, automatic: false },
        "acct-side": side,
      },
    });
    renderSettings({ onSettingsChange, resetAlerts: rendered.resetAlerts });

    const list = screen.getByRole("list", { name: "Accounts with a banked reset alert" });
    expect(list).toHaveTextContent("work@example.com · 5% or less left, 24h or more before it renews · tells you");
    expect(list).toHaveTextContent("Codex account · 3% or less left, 48h or more before it renews · uses it by itself");

    await user.click(screen.getByRole("button", { name: "Turn off the banked reset alert for work@example.com" }));

    expect(onSettingsChange).toHaveBeenCalledTimes(1);
    expect(lastChangeTo(onSettingsChange, rendered)).toEqual({ resetAlerts: { "acct-side": side } });
    expect(lastChangeTo(onSettingsChange, { ...rendered, resetAlerts: { "acct-work": side, "acct-new": side } })).toEqual({
      resetAlerts: { "acct-new": side },
    });
  });

  it("lists no account while no banked reset alert is on", () => {
    renderSettings();

    expect(screen.getByRole("region", { name: "Banked reset alerts" })).toBeTruthy();
    expect(screen.queryByRole("list", { name: "Accounts with a banked reset alert" })).toBeNull();
  });
});
