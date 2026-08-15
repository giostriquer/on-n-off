import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "./AppShell";

const setFilter = vi.hoisted(() => vi.fn());
const navigate = vi.hoisted(() => vi.fn());

const agent = {
  id: "codex" as const,
  displayName: "Codex",
  cliOk: true,
  cliError: null,
  installGit: true,
  installFolder: true,
  pluginToggle: true,
};

const session = {
  theme: "dark" as const,
  setTheme: vi.fn(),
  visibleAgents: [agent],
  selected: "codex" as const,
  setSelected: vi.fn(),
  currentAgent: agent,
  currentTab: {
    dto: { plugins: [], userSkills: [], mcpServers: [] },
    filter: "workbench",
    inFlight: false,
    loading: false,
  },
  banner: null,
  canInstall: false,
  counts: {
    plugins: { on: 0, total: 0 },
    skills: { on: 0, total: 0 },
    mcp: { on: 0, total: 0 },
  },
  allOn: false,
  cliLine: "codex · ~/.codex",
  masterNote: "cuts every item for Codex",
  showMasterCut: false,
  currentProjects: [],
  currentScopePath: null,
  scopeNote: "global agent config is the source of truth",
  installOpen: false,
  setInstallOpen: vi.fn(),
  installError: null,
  clearInstallError: vi.fn(),
  uninstallTarget: null,
  setUninstallTarget: vi.fn(),
  setFilter,
  loadTab: vi.fn(),
  selectScope: vi.fn(),
  pickProjectFolder: vi.fn(),
  openProjectPath: vi.fn(),
  masterCut: vi.fn(),
  pickFolder: vi.fn(),
  install: vi.fn(),
  confirmUninstall: vi.fn(),
};

vi.mock("@tanstack/react-router", () => ({
  Outlet: () => null,
  useNavigate: () => navigate,
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => string }) =>
    select({ location: { pathname: "/plugins" } }),
}));

vi.mock("@/features/session/SessionProvider", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/features/session/SessionProvider")>();
  return { ...actual, useAgentSession: () => session };
});

vi.mock("@/features/agents/AgentBanner", () => ({ AgentBanner: () => null }));
vi.mock("@/features/catalog/ConfirmDialog", () => ({ ConfirmDialog: () => null }));
vi.mock("@/features/catalog/InstallSheet", () => ({ InstallSheet: () => null }));
vi.mock("@/features/scope/ScopeBar", () => ({ ScopeBar: () => null }));
vi.mock("@/features/shell/LeftRail", () => ({ LeftRail: () => null }));
vi.mock("@/features/updater/UpdateStrip", () => ({ UpdateStrip: () => null }));
vi.mock("@/features/usage/LazyUsageChart", () => ({ preloadUsageChart: vi.fn() }));

describe("AppShell filter", () => {
  beforeEach(() => {
    for (const name of ["localStorage", "sessionStorage"] as const) {
      const values = new Map<string, string>();
      Object.defineProperty(window, name, {
        configurable: true,
        value: {
          getItem: (key: string) => values.get(key) ?? null,
          setItem: (key: string, value: string) => values.set(key, String(value)),
          removeItem: (key: string) => values.delete(key),
          clear: () => values.clear(),
        },
      });
    }
    setFilter.mockClear();
  });

  it("applies input changes and clears the active provider filter with Escape", () => {
    render(<AppShell />);
    const input = screen.getByRole("searchbox");

    expect(input).toHaveValue("workbench");
    fireEvent.change(input, { target: { value: "brainstorm" } });
    expect(setFilter).toHaveBeenLastCalledWith("brainstorm");

    fireEvent.keyDown(input, { key: "Escape" });
    expect(setFilter).toHaveBeenLastCalledWith("");
  });
});
