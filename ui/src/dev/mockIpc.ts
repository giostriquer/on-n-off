/**
 * Dev-only stand-in for Tauri's IPC so the UI can run in a plain browser (and under the
 * screenshot harness, `bun run ui:shots`). Loaded by `main.tsx` only in dev builds and only when
 * the page URL carries `?mock[=scenario]`; production bundles never include it.
 *
 * Every command answers with synthetic data. Unknown commands reject with a visible message so
 * a screen that reaches for something unmocked says so instead of hanging.
 */

import type { AppSettings, AgentInfo, AgentId } from "$lib/types";
import { SCENARIOS } from "./githubFixtures";
import { limitsFor } from "./limitsFixtures";
import { defaultNotchSettings, type NotchSnapshot, type NotchSettings } from "$lib/notchTypes";
import type { UsageBucket, UsageSummary } from "$lib/usageTypes";

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
  limitsPollMinutes: 5,
  githubScopes: ["org:acme", "repo:octo/tools"],
  githubNotifications: false,
  githubPollSeconds: 60,
  closeToTray: false,
};

const emptyTab = () => ({ plugins: [], userSkills: [], mcpServers: [] });

let notch: NotchSnapshot = {
  revision: 0,
  supported: true,
  settings: defaultNotchSettings({ enabled: true, displayId: "studio" }),
  displays: [
    { id: "built-in", name: "Built-in Retina Display", x: -1728, y: 0, width: 1728, height: 1117, workY: 33, workHeight: 1084, scale: 2, mirrored: false },
    { id: "studio", name: "Studio Display", x: 0, y: 0, width: 2560, height: 1440, workY: 25, workHeight: 1415, scale: 2, mirrored: false },
  ],
  error: null,
};

/** A month of usage with five-digit model totals, so the Overview's cost columns are exercised. */
function usageSummaryFor(input: { sinceDay: string; untilDay: string; timeZone: string }): UsageSummary {
  const totals = (output: number) => ({
    uncachedInputTokens: output * 6,
    cachedInputTokens: output * 260,
    cacheCreationTokens: output * 2,
    outputTokens: output,
    reasoningTokens: Math.round(output * 0.28),
  });
  const models: [AgentId, string, number][] = [
    ["codex", "gpt-5.6-sol", 10969.67],
    ["claude", "claude-fable-5", 4282.86],
    ["claude", "claude-opus-5", 1589.63],
    ["claude", "claude-opus-4-8", 3.34],
  ];
  const until = new Date(`${input.untilDay}T00:00:00Z`);
  const buckets: UsageBucket[] = [];
  for (let back = 15; back >= 0; back -= 1) {
    const day = new Date(until.getTime() - back * 86_400_000).toISOString().slice(0, 10);
    if (day < input.sinceDay) continue;
    for (const [provider, model, monthly] of models) {
      const costUsd = Math.round((monthly / 16) * (1 + Math.sin(back)) * 100) / 100;
      buckets.push({
        day, provider, model, totals: totals(Math.round(costUsd * 1200)), costUsd,
        cacheSavingsUsd: costUsd * 1.3, costSource: "modelPriced", records: 40, unpricedRecords: 0, sessions: 3,
      });
    }
  }
  return {
    readAt: `${input.untilDay}T12:00:00Z`, timeZone: input.timeZone, sinceDay: input.sinceDay, untilDay: input.untilDay,
    buckets,
    sources: [
      { provider: "claude", status: "ok", scannedFiles: 120, skippedFiles: 0, malformedRecords: 0, distinctSessions: 40, resolvedPath: "~/.claude/projects" },
      { provider: "codex", status: "ok", scannedFiles: 60, skippedFiles: 0, malformedRecords: 0, distinctSessions: 20, resolvedPath: "~/.codex/sessions" },
    ],
    pricing: { status: "fresh", source: "litellm", fetchedAt: `${input.untilDay}T11:00:00Z`, knownModels: 900 },
    scanDurationMs: 12,
    cacheHit: true,
  };
}

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
  read_limits: (args) => limitsFor(args.agentId),
  usage_summary: (args) => usageSummaryFor(args.input as { sinceDay: string; untilDay: string; timeZone: string }),
  read_notch_state: () => notch,
  save_notch_settings: (args) => { notch = { ...notch, settings: args.settings as NotchSettings }; return notch; },
  hide_limits_popover: () => undefined,
  open_limits_window: () => undefined,
  quit_app: () => undefined,
  tray_supported: () => true,
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
