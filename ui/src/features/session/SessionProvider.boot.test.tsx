import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { AgentId, AgentInfo, AgentTabDto } from "$lib/types";
import { AppSettingsProvider } from "$lib/useAppSettings";
import { AGENT_KEY, SessionProvider, useAgentSession } from "./SessionProvider";

const state = vi.hoisted(() => ({ loaded: [] as AgentId[] }));

vi.mock("$lib/appSettings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/appSettings")>();
  return { ...actual, DEFAULT_APP_SETTINGS: { ...actual.DEFAULT_APP_SETTINGS, hiddenAgents: ["codex"] } };
});

const EMPTY_TAB: AgentTabDto = { plugins: [], userSkills: [], mcpServers: [] };
const AGENTS: AgentInfo[] = (["claude", "codex", "antigravity", "cursor", "muse"] as const).map((id) => ({
  id,
  displayName: id,
  cliOk: true,
  cliError: null,
  installGit: true,
  installFolder: true,
  pluginToggle: true,
  readsHooks: false,
}));

vi.mock("$lib/api", () => ({
  featureFlags: () => Promise.resolve({ masterCut: false }),
  loadAppSettings: () => Promise.reject(new Error("settings unavailable")),
  listAgents: () => Promise.resolve(AGENTS),
  listProjects: () => Promise.resolve([]),
  listLocalPlugins: (agentId: AgentId) => {
    state.loaded.push(agentId);
    return Promise.resolve(EMPTY_TAB);
  },
  listPlugins: () => Promise.resolve(EMPTY_TAB),
  refresh: () => Promise.resolve(EMPTY_TAB),
}));

function Selected() {
  return <span data-testid="selected-provider">{useAgentSession().selected}</span>;
}

beforeEach(() => {
  localStorage.clear();
  state.loaded.length = 0;
});

it("boots on the remembered provider when the settings could not be read, whatever the placeholder hides", async () => {
  localStorage.setItem(AGENT_KEY, "codex");

  render(
    <SessionProvider>
      <Selected />
    </SessionProvider>,
    { wrapper: AppSettingsProvider },
  );

  await waitFor(() => expect(state.loaded.length).toBe(AGENTS.length));
  expect(state.loaded[0]).toBe("codex");
  expect(screen.getByTestId("selected-provider")).toHaveTextContent("codex");
});
