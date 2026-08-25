/**
 * Dev-only stand-in for Tauri's IPC so the UI can run in a plain browser (and under the
 * screenshot harness, `bun run ui:shots`). Loaded by `main.tsx` only in dev builds and only when
 * the page URL carries `?mock[=scenario]`; production bundles never include it.
 *
 * Every command answers with synthetic data. Unknown commands reject with a visible message so
 * a screen that reaches for something unmocked says so instead of hanging.
 */

import type { AppSettings, AgentInfo } from "$lib/types";
import { SCENARIOS } from "./githubFixtures";

type Handler = (args: Record<string, unknown>) => unknown;

declare global {
  interface Window {
    __TAURI_INTERNALS__?: {
      invoke: (cmd: string, args?: Record<string, unknown>, options?: unknown) => Promise<unknown>;
      transformCallback: (callback?: (payload: unknown) => void, once?: boolean) => number;
      convertFileSrc: (path: string, protocol?: string) => string;
      metadata: { currentWindow: { label: string }; currentWebview: { windowLabel: string; label: string } };
    };
  }
}

const params = new URLSearchParams(window.location.search);
const scenario = params.get("mock") || "ok";
const latency = Number(params.get("latency") ?? 80);
if (!Object.hasOwn(SCENARIOS, scenario)) {
  console.error(`[mock] unknown github scenario "${scenario}"; known: ${Object.keys(SCENARIOS).join(", ")}`);
}

const AGENTS: AgentInfo[] = [
  { id: "claude", displayName: "Claude", cliOk: true, cliError: null, installGit: true, installFolder: true, pluginToggle: true },
  { id: "codex", displayName: "Codex", cliOk: true, cliError: null, installGit: true, installFolder: true, pluginToggle: true },
  { id: "antigravity", displayName: "Antigravity", cliOk: false, cliError: "Antigravity CLI not found.", installGit: false, installFolder: false, pluginToggle: false },
  { id: "cursor", displayName: "Cursor", cliOk: false, cliError: "Cursor CLI not found.", installGit: false, installFolder: false, pluginToggle: false },
];

let settings: AppSettings = {
  hiddenAgents: ["antigravity", "cursor"],
  binaryPaths: {},
  automaticUpdates: false,
  limitNotifications: false,
  limitsPollMinutes: 10,
  githubScopes: ["org:acme", "repo:octo/tools"],
  githubNotifications: false,
  githubPollSeconds: 60,
};

const emptyTab = () => ({ plugins: [], userSkills: [], mcpServers: [] });

const handlers: Record<string, Handler> = {
  list_agents: () => AGENTS,
  feature_flags: () => ({ masterCut: false }),
  updater_build_info: () => ({ enabled: false, installerKind: null, target: null }),
  load_app_settings: () => settings,
  save_app_settings: (args) => {
    settings = args.settings as AppSettings;
    return settings;
  },
  request_notification_permission: () => true,
  diagnose_providers: () => [],
  list_projects: () => [],
  list_plugins: emptyTab,
  list_local_plugins: emptyTab,
  refresh: emptyTab,
  read_limits: () => [],
  read_github_prs: () => {
    if (!Object.hasOwn(SCENARIOS, scenario)) {
      throw { kind: "message", message: `mock: unknown scenario "${scenario}"`, path: null };
    }
    return SCENARIOS[scenario]();
  },
  open_url: (args) => {
    console.info("[mock] open_url", args.url);
  },
  "plugin:app|version": () => "0.0.0-mock",
  "plugin:event|listen": () => 1,
  "plugin:event|unlisten": () => undefined,
  "plugin:dialog|open": () => null,
};

window.__TAURI_INTERNALS__ = {
  async invoke(cmd, args = {}) {
    const handler = Object.hasOwn(handlers, cmd) ? handlers[cmd] : undefined;
    if (!handler) {
      // Loud on purpose: the screenshot harness fails a scene on console errors.
      console.error(`[mock] no handler for ${cmd}`);
      throw { kind: "message", message: `mock IPC has no handler for ${cmd}`, path: null };
    }
    await new Promise((resolve) => setTimeout(resolve, latency));
    try {
      return await handler(args);
    } catch (error) {
      console.error(`[mock] ${cmd} failed:`, error);
      throw error;
    }
  },
  transformCallback(callback, once = false) {
    const id = Math.floor(Math.random() * 1_000_000);
    const key = `_${id}` as keyof Window;
    Object.defineProperty(window, key, {
      value: (payload: unknown) => {
        if (once) delete (window as unknown as Record<string, unknown>)[key];
        callback?.(payload);
      },
      writable: false,
      configurable: true,
    });
    return id;
  },
  convertFileSrc: (path) => path,
  metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
};

window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };

console.info(`[mock] Tauri IPC mocked · github scenario "${scenario}" · latency ${latency} ms`);
