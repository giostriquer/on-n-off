import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_APP_SETTINGS } from "$lib/appSettings";
import { defaultNotchSettings } from "$lib/notchTypes";
import type { AgentInfo, AgentTabDto, AppSettings } from "$lib/types";
import { AGENT_KEY, SessionProvider, useAgentSession } from "@/features/session/SessionProvider";
import { UpdateProvider } from "@/features/updater/UpdateProvider";
import type { UpdaterClient } from "@/features/updater/updaterClient";
import { SettingsRoute } from "./settings";

const LOADED: AppSettings = {
  ...DEFAULT_APP_SETTINGS,
  binaryPaths: { claude: "/opt/acme/bin/claude" },
  limitsPollMinutes: 10,
  githubScopes: ["org:acme"],
};

const EMPTY_TAB: AgentTabDto = { plugins: [], userSkills: [], mcpServers: [] };

const AGENTS: AgentInfo[] = (["claude", "codex", "antigravity", "cursor", "muse"] as const).map((id) => ({
  id,
  displayName: id === "claude" ? "Claude" : id === "codex" ? "Codex" : id,
  cliOk: true,
  cliError: null,
  installGit: true,
  installFolder: true,
  pluginToggle: true,
  readsHooks: false,
}));

const api = vi.hoisted(() => ({
  loadAppSettings: vi.fn<() => Promise<AppSettings>>(),
  saveAppSettings: vi.fn<(settings: AppSettings) => Promise<AppSettings>>(),
  listAgents: vi.fn<() => Promise<AgentInfo[]>>(),
}));

vi.mock("$lib/api", () => ({
  featureFlags: () => Promise.resolve({ masterCut: false }),
  loadAppSettings: api.loadAppSettings,
  saveAppSettings: api.saveAppSettings,
  listAgents: api.listAgents,
  listProjects: () => Promise.resolve([]),
  listLocalPlugins: () => Promise.resolve(EMPTY_TAB),
  listPlugins: () => Promise.resolve(EMPTY_TAB),
  refresh: () => Promise.resolve(EMPTY_TAB),
  readAccountPreferences: () => Promise.resolve(false),
  accountAction: () => Promise.resolve(),
  onSharedReadChanged: () => Promise.resolve(() => undefined),
  readNotchState: () =>
    Promise.resolve({ revision: 0, supported: false, liquidGlass: false, settings: defaultNotchSettings(), displays: [], error: null }),
  onNotchChanged: () => Promise.resolve(() => undefined),
  saveNotchSettings: vi.fn(),
  usageHistoryStatus: () => Promise.resolve({ state: "empty", bytes: 0 }),
  clearUsageHistory: vi.fn(),
  diagnoseProviders: () => Promise.resolve([]),
  requestNotificationPermission: () => Promise.resolve(true),
  traySupported: () => Promise.resolve(true),
}));

const updaterClient: UpdaterClient = {
  buildInfo: async () => ({ enabled: false, installerKind: null, target: null }),
  currentVersion: async () => "0.1.0",
  check: async () => null,
  relaunch: async () => undefined,
};

function SessionProbe() {
  const session = useAgentSession();
  return (
    <>
      <span data-testid="selected-provider">{session.selected}</span>
      <span data-testid="log">{session.log.map((entry) => `${entry.tag} ${entry.text}`).join(" | ")}</span>
    </>
  );
}

function Window({ children }: { children: ReactNode }) {
  return <SessionProvider>{children}</SessionProvider>;
}

async function renderSettings() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <Window>
        <UpdateProvider initialProviderReady={false} automaticUpdates client={updaterClient}>
          <SettingsRoute />
          <SessionProbe />
        </UpdateProvider>
      </Window>
    </QueryClientProvider>,
  );
  await waitFor(() => expect(screen.getByRole("combobox", { name: "Limits polling interval" })).toHaveValue("10"));
}

function lastSaved(): AppSettings {
  return api.saveAppSettings.mock.lastCall![0];
}

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem(AGENT_KEY, "claude");
  api.loadAppSettings.mockReset().mockResolvedValue(LOADED);
  api.saveAppSettings.mockReset().mockImplementation(async (settings) => settings);
  api.listAgents.mockReset().mockResolvedValue(AGENTS);
});

describe("the Settings route", () => {
  it("saves a change with the rest of the document it loaded", async () => {
    const user = userEvent.setup();
    await renderSettings();

    const toggle = await screen.findByRole("button", { name: "Keep on-n-off running in the tray when the window is closed" });
    await user.click(toggle);

    expect(api.saveAppSettings).toHaveBeenCalledTimes(1);
    expect(lastSaved()).toEqual({ ...LOADED, closeToTray: true });
    await waitFor(() => expect(toggle).toHaveAttribute("aria-pressed", "true"));
  });

  it("saves automatic updates, limit notifications and the poll interval the same way", async () => {
    const user = userEvent.setup();
    await renderSettings();

    await user.click(screen.getByRole("button", { name: "Automatically download updates" }));
    await waitFor(() => expect(lastSaved()).toEqual({ ...LOADED, automaticUpdates: false }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Automatically download updates" })).toHaveAttribute("aria-pressed", "false"),
    );

    await user.selectOptions(screen.getByRole("combobox", { name: "Limits polling interval" }), "15");
    await waitFor(() => expect(lastSaved()).toEqual({ ...LOADED, automaticUpdates: false, limitsPollMinutes: 15 }));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Limits polling interval" })).toHaveValue("15"));

    await user.click(screen.getByRole("button", { name: "Notify about limit changes" }));
    await waitFor(() =>
      expect(lastSaved()).toEqual({ ...LOADED, automaticUpdates: false, limitsPollMinutes: 15, limitNotifications: true }),
    );
  });

  it("hides the selected provider and moves the session to the first one still shown", async () => {
    localStorage.setItem(AGENT_KEY, "codex");
    const user = userEvent.setup();
    await renderSettings();
    expect(screen.getByTestId("selected-provider")).toHaveTextContent("codex");

    await user.click(screen.getByRole("button", { name: "Show Codex in agent tabs" }));

    expect(lastSaved()).toEqual({ ...LOADED, hiddenAgents: ["codex"] });
    await waitFor(() => expect(screen.getByTestId("selected-provider")).toHaveTextContent("claude"));
  });

  it("saves a binary path trimmed and reads the providers again", async () => {
    await renderSettings();
    const before = api.listAgents.mock.calls.length;

    const codex = screen.getByPlaceholderText("codex");
    fireEvent.change(codex, { target: { value: "  /opt/acme/bin/codex  " } });
    fireEvent.blur(codex);

    await waitFor(() =>
      expect(lastSaved()).toEqual({
        ...LOADED,
        binaryPaths: { claude: "/opt/acme/bin/claude", codex: "/opt/acme/bin/codex" },
      }),
    );
    await waitFor(() => expect(api.listAgents.mock.calls.length).toBe(before + 1));
  });

  it("removes a binary path left blank", async () => {
    await renderSettings();

    const claude = screen.getByPlaceholderText("claude");
    expect(claude).toHaveValue("/opt/acme/bin/claude");
    fireEvent.change(claude, { target: { value: "   " } });
    fireEvent.blur(claude);

    await waitFor(() => expect(lastSaved()).toEqual({ ...LOADED, binaryPaths: {} }));
  });

  it("notes a refused save in the session log, keeps the document and reads no providers again", async () => {
    api.saveAppSettings.mockRejectedValue({ kind: "message", message: "Keep at least one provider visible in the agent tabs.", path: null });
    await renderSettings();
    const before = api.listAgents.mock.calls.length;

    const codex = screen.getByPlaceholderText("codex");
    fireEvent.change(codex, { target: { value: "/opt/acme/bin/codex" } });
    fireEvent.blur(codex);
    await waitFor(() => expect(screen.getByTestId("log")).toHaveTextContent("TRIP Keep at least one provider visible in the agent tabs."));

    const toggle = screen.getByRole("button", { name: "Keep on-n-off running in the tray when the window is closed" });
    fireEvent.click(toggle);
    await waitFor(() => expect(api.saveAppSettings).toHaveBeenCalledTimes(2));
    expect(lastSaved()).toEqual({ ...LOADED, closeToTray: true });
    await waitFor(() =>
      expect(screen.getByTestId("log").textContent?.match(/Keep at least one provider visible/g)).toHaveLength(2),
    );
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    expect(api.listAgents.mock.calls.length).toBe(before);
  });

  it("shows the default settings when they cannot be read", async () => {
    api.loadAppSettings.mockRejectedValue(new Error("settings unavailable"));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <Window>
          <UpdateProvider initialProviderReady={false} automaticUpdates client={updaterClient}>
            <SettingsRoute />
          </UpdateProvider>
        </Window>
      </QueryClientProvider>,
    );

    await waitFor(() => expect(api.loadAppSettings).toHaveBeenCalled());
    expect(screen.getByRole("combobox", { name: "Limits polling interval" })).toHaveValue("5");
    expect(screen.getByRole("button", { name: "Automatically download updates" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByPlaceholderText("claude")).toHaveValue("");
  });
});
