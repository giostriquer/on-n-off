import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Settings } from "./Settings";
import { UpdateProvider } from "@/features/updater/UpdateProvider";
import type { AgentInfo } from "$lib/types";
import type { UpdaterClient } from "@/features/updater/updaterClient";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => Promise.resolve(null),
}));

vi.mock("$lib/api", () => ({
  diagnoseProviders: () =>
    Promise.resolve([
      {
        agentId: "claude",
        binary: "claude",
        homePath: "C:\\Users\\me\\.claude",
        checks: [
          { id: "cli", label: "CLI binary", ok: false, detail: "claude is not on PATH", hint: "Set a binary path." },
        ],
      },
    ]),
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
];

const updaterClient: UpdaterClient = {
  buildInfo: async () => ({ enabled: false, installerKind: null, target: null }),
  currentVersion: async () => "0.1.0",
  check: async () => null,
  relaunch: async () => undefined,
};

describe("Settings", () => {
  it("renders provider cards and can hide one from tabs", async () => {
    const user = userEvent.setup();
    const onToggleVisible = vi.fn();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <UpdateProvider initialProviderReady automaticUpdates client={updaterClient}>
          <Settings
            agents={agents}
            settings={{ hiddenAgents: [], binaryPaths: {}, automaticUpdates: true }}
            onToggleVisible={onToggleVisible}
            onSaveBinary={() => undefined}
            onAutomaticUpdatesChange={() => undefined}
          />
        </UpdateProvider>
      </QueryClientProvider>,
    );
    expect(screen.getAllByText("Claude").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Codex").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /Show Claude in agent tabs/i })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Show Antigravity in agent tabs/i }));
    expect(onToggleVisible).toHaveBeenCalledWith("antigravity", true);
  });
});
