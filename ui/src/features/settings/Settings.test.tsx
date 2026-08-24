import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Settings } from "./Settings";
import { UpdateProvider } from "@/features/updater/UpdateProvider";
import type { AgentInfo, AppSettings, LimitsPollMinutes } from "$lib/types";
import type { UpdaterClient } from "@/features/updater/updaterClient";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => Promise.resolve(null),
}));

const apiMocks = vi.hoisted(() => ({
  requestNotificationPermission: vi.fn<() => Promise<boolean>>(),
}));

vi.mock("$lib/api", () => ({
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
  },
  {
    id: "codex",
    displayName: "Codex",
    cliOk: true,
    cliError: null,
    installGit: true,
    installFolder: true,
    pluginToggle: true,
  },
  {
    id: "antigravity",
    displayName: "Antigravity",
    cliOk: false,
    cliError: null,
    installGit: false,
    installFolder: false,
    pluginToggle: false,
  },
  {
    id: "cursor",
    displayName: "Cursor",
    cliOk: false,
    cliError: null,
    installGit: false,
    installFolder: false,
    pluginToggle: false,
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
  onLimitNotificationsChange = () => undefined,
  onLimitsPollMinutesChange = () => undefined,
  onGithubChange = () => undefined,
  githubScopes = [],
}: {
  onToggleVisible?: (id: AgentInfo["id"], hidden: boolean) => void;
  onLimitNotificationsChange?: (enabled: boolean) => void;
  onLimitsPollMinutesChange?: (minutes: LimitsPollMinutes) => void;
  onGithubChange?: (patch: Partial<AppSettings>) => void;
  githubScopes?: string[];
} = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <UpdateProvider initialProviderReady automaticUpdates client={updaterClient}>
        <Settings
          agents={agents}
          settings={{
            hiddenAgents: [],
            binaryPaths: {},
            automaticUpdates: true,
            limitNotifications: false,
            limitsPollMinutes: 10,
            githubScopes,
            githubNotifications: false,
            githubPollSeconds: 60,
          }}
          onToggleVisible={onToggleVisible}
          onSaveBinary={() => undefined}
          onAutomaticUpdatesChange={() => undefined}
          onLimitNotificationsChange={onLimitNotificationsChange}
          onLimitsPollMinutesChange={onLimitsPollMinutesChange}
          onGithubChange={onGithubChange}
        />
      </UpdateProvider>
    </QueryClientProvider>,
  );
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

  it("uses Cursor's `agent` command as the binary placeholder", () => {
    renderSettings();

    expect(screen.getByPlaceholderText("agent")).toBeTruthy();
    expect(screen.queryByPlaceholderText("cursor-agent")).toBeNull();
  });

  it("requests notification permission before enabling limit polling and persists the interval", async () => {
    const user = userEvent.setup();
    const onLimitNotificationsChange = vi.fn();
    const onLimitsPollMinutesChange = vi.fn();
    renderSettings({ onLimitNotificationsChange, onLimitsPollMinutesChange });

    await user.click(screen.getByRole("button", { name: "Notify about limit changes" }));

    expect(apiMocks.requestNotificationPermission).toHaveBeenCalledTimes(1);
    expect(onLimitNotificationsChange).toHaveBeenCalledWith(true);

    await user.selectOptions(
      screen.getByRole("combobox", { name: "Limits polling interval" }),
      "15",
    );
    expect(onLimitsPollMinutesChange).toHaveBeenCalledWith(15);
  });

  it("keeps limit polling disabled when notification permission is denied", async () => {
    apiMocks.requestNotificationPermission.mockResolvedValue(false);
    const user = userEvent.setup();
    const onLimitNotificationsChange = vi.fn();
    renderSettings({ onLimitNotificationsChange });

    await user.click(screen.getByRole("button", { name: "Notify about limit changes" }));

    expect(onLimitNotificationsChange).not.toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Notifications are blocked in system settings.",
    );
  });

  it("adds and removes GitHub scopes with Enter and the chip button, never on blur", async () => {
    const user = userEvent.setup();
    const onGithubChange = vi.fn();
    renderSettings({ onGithubChange, githubScopes: ["org:acme", "repo:me/tool"] });

    const card = screen.getByRole("region", { name: "Pull requests" });
    expect(card).toHaveTextContent("org:acme");
    expect(card).toHaveTextContent("repo:me/tool");

    const input = screen.getByRole("textbox", { name: "Scopes" });
    expect(input.getAttribute("aria-describedby")).toBeTruthy();
    await user.type(input, "  user:me{Enter}");
    expect(onGithubChange).toHaveBeenCalledWith({ githubScopes: ["org:acme", "repo:me/tool", "user:me"] });
    expect(input).toHaveValue("");

    await user.click(screen.getByRole("button", { name: "Remove org:acme" }));
    expect(onGithubChange).toHaveBeenCalledWith({ githubScopes: ["repo:me/tool"] });

    onGithubChange.mockClear();
    await user.type(input, "   {Enter}");
    expect(onGithubChange).not.toHaveBeenCalled();

    await user.type(input, "org:acme{Enter}");
    expect(onGithubChange).not.toHaveBeenCalled();
    expect(input).toHaveValue("");

    // Leaving the field keeps the draft: a blur-commit would race the chip's Remove click.
    await user.type(input, "org:other");
    await user.tab();
    expect(onGithubChange).not.toHaveBeenCalled();
    expect(input).toHaveValue("org:other");
  });

  it("requests notification permission before enabling CI notifications and persists the interval", async () => {
    const user = userEvent.setup();
    const onGithubChange = vi.fn();
    renderSettings({ onGithubChange });

    await user.click(screen.getByRole("button", { name: "Notify about CI changes" }));

    expect(apiMocks.requestNotificationPermission).toHaveBeenCalledTimes(1);
    expect(onGithubChange).toHaveBeenCalledWith({ githubNotifications: true });

    await user.selectOptions(screen.getByRole("combobox", { name: "GitHub polling interval" }), "120");
    expect(onGithubChange).toHaveBeenCalledWith({ githubPollSeconds: 120 });
  });

  it("keeps CI notifications disabled when notification permission is denied", async () => {
    apiMocks.requestNotificationPermission.mockResolvedValue(false);
    const user = userEvent.setup();
    const onGithubChange = vi.fn();
    renderSettings({ onGithubChange });

    await user.click(screen.getByRole("button", { name: "Notify about CI changes" }));

    expect(onGithubChange).not.toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent("Notifications are blocked in system settings.");
  });
});
