import { subscriptionBadgeLimits, subscriptionBadgeProfiles, subscriptionBadgeReading } from "./subscriptionFixtures";
/**
 * Dev-only stand-in for Tauri's IPC so the UI can run in a plain browser (and under the
 * screenshot harness, `bun run ui:shots`). Loaded by `main.tsx` only in dev builds and only when
 * the page URL carries `?mock[=scenario]`; production bundles never include it.
 *
 * Every command answers with synthetic data. Unknown commands reject with a visible message so
 * a screen that reaches for something unmocked says so instead of hanging.
 */

import type { AppSettings, AgentInfo, AgentId, AgentTabDto } from "$lib/types";
import { SCENARIOS } from "./githubFixtures";
import { hooksFor } from "./hooksFixtures";
import { bankedResetsClaude, bankedResetsCodex, claudeSubscriptionStatusClaude, claudeWithoutReset, creditsSpentCodex, limitsBandClaude, limitsBandCodex, limitsFor, limitsOrderClaude, sameEmailWorkspacesCodex, workspaceCreditsCodex } from "./limitsFixtures";
import { defaultNotchSettings, type NotchSnapshot, type NotchSettings } from "$lib/notchTypes";
import type { UsageBucket, UsageHistoryStatus, UsageSummary } from "$lib/usageTypes";

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
// Scenarios this file answers for itself. `SCENARIOS` holds the pull-request ones.
const LOCAL_SCENARIOS = [
  "subscriptionRenewal", "subscriptionStale", "subscriptionMissing", "accountLogin", "accountLocked",
  "accountDuplicate", "accountClients", "billingFailure", "claudeMissingReset", "subscriptionBadges", "catalog",
  "savedRefreshPaused", "limitsBand", "limitsOrder", "bankedResets", "sameEmailWorkspaces", "workspaceCredits", "creditsSpent", "claudeSubscriptionStatus", "hooks", "mcpSources",
];
if (!Object.hasOwn(SCENARIOS, scenario) && !LOCAL_SCENARIOS.includes(scenario)) {
  console.error(
    `[mock] unknown scenario "${scenario}"; known: ${[...Object.keys(SCENARIOS), ...LOCAL_SCENARIOS].join(", ")}`,
  );
}

const vaultDenied = new Error("Could not unlock saved accounts.");
const billingFailure = new Error("Could not read a matching browser session. Check browser cookie access, then retry.");
let vaultLocked = scenario === "accountLocked";
let duplicateReconnected = false;
const pendingLogins = new Map<string, () => void>();

const AGENTS: AgentInfo[] = [
  { id: "claude", displayName: "Claude", cliOk: true, cliError: null, installGit: true, installFolder: true, pluginToggle: true, readsHooks: true },
  { id: "codex", displayName: "Codex", cliOk: true, cliError: null, installGit: true, installFolder: true, pluginToggle: true, readsHooks: true },
  { id: "antigravity", displayName: "Antigravity", cliOk: false, cliError: "Antigravity CLI not found.", installGit: false, installFolder: false, pluginToggle: false, readsHooks: false },
  { id: "cursor", displayName: "Cursor", cliOk: false, cliError: "Cursor CLI not found.", installGit: false, installFolder: false, pluginToggle: false, readsHooks: false },
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

const emptyTab = (): AgentTabDto => ({ plugins: [], userSkills: [], mcpServers: [], hooks: [] });

// `?mock=catalog`: a real-sized catalog. The Overview's live list is the one surface whose layout
// only misbehaves once it is long, so an empty tab cannot stand in for it.
const CATALOG_PLUGINS = [
  "workbench", "mattpocock-skills", "toolkit", "linear", "design", "dataviz", "artifact-design",
  "artifact-capabilities", "update-config", "keybindings-help", "code-review", "simplify",
  "schedule", "claude-api",
];
const CATALOG_SKILLS = [
  "adopt-global-rules", "arch-map", "audit", "brainstorming", "claim-check", "code-quality-review",
  "context7-mcp", "documents", "empirical-proof", "epic-implementation", "file-pr", "fix-ci",
  "grilling", "model-reference", "qa-sweep", "receiving-code-review", "route-work",
  "systematic-debugging", "test-driven-development", "using-workbench",
  "verification-before-completion", "diagnosing-bugs", "domain-modeling", "codebase-design",
  "prototype", "research", "resolving-merge-conflicts", "tdd", "wizard", "writing-for-agents",
  "ui-demo-video", "get-pr-comments",
];
const CATALOG_MCPS = ["context7", "linear", "playwright", "sentry", "postgres"];

const fullTab = (): AgentTabDto => ({
  plugins: CATALOG_PLUGINS.map((name, index) => ({
    id: `${name}@workshop`,
    name,
    source: "workshop",
    version: `0.${index + 12}.0`,
    upstream: `0.${index + 12}.0`,
    enabled: true,
    togglable: true,
    // The first plugin ships skills of its own. Those rows are not togglable, so they render the
    // "with plugin" span rather than a Rocker — the variant whose height has to match it.
    skills: index === 0
      ? ["dispatch", "handoff"].map((skill) => ({
          id: `${name}:${skill}`,
          pluginId: `${name}@workshop`,
          name: `${name}:${skill}`,
          description: `${skill} from ${name}`,
          enabled: true,
          togglable: false,
        }))
      : [],
  })),
  userSkills: CATALOG_SKILLS.map((name) => ({
    id: name,
    pluginId: null,
    name,
    description: `${name} skill`,
    enabled: true,
    togglable: true,
  })),
  // Two are off, so the gauge reads 3 on / 5 installed.
  mcpServers: CATALOG_MCPS.map((name, index) => ({
    id: name,
    name,
    system: "npx",
    source: `npx -y @modelcontextprotocol/server-${name}`,
    enabled: index < 3,
    togglable: true,
  })),
});

// `?mock=mcpSources`: the catalog plus the Claude servers that are not the user's own — one the
// toolkit plugin brings and two kept for particular projects.
const mcpSourcesTab = (): AgentTabDto => {
  const tab = fullTab();
  return {
    ...tab,
    mcpServers: [
      ...tab.mcpServers,
      {
        id: "plugin:toolkit:tracker",
        name: "tracker",
        system: "http",
        source: "https://tracker.example/mcp",
        enabled: true,
        togglable: false,
        origin: "plugin",
        pluginId: "toolkit@workshop",
      },
      {
        id: "local:library-docs",
        name: "library-docs",
        system: "http",
        source: "https://docs.example/mcp",
        enabled: true,
        togglable: false,
        origin: "local",
        projects: Array.from({ length: 18 }, (_, index) => `/Users/me/acme/app-${String(index + 1).padStart(2, "0")}`),
      },
      {
        id: "local:scratchpad",
        name: "scratchpad",
        system: "stdio",
        source: "node pad.js",
        enabled: true,
        togglable: false,
        origin: "local",
        projects: ["/Users/me/acme/webapp"],
      },
    ],
  };
};

// `?mock=hooks`: the Hooks screen's rows, which are per provider — Antigravity and Cursor get
// none, and say so rather than reading as unconfigured.
const catalogTab = (args: Record<string, unknown> = {}): AgentTabDto => {
  const tab = scenario === "catalog" ? fullTab() : scenario === "mcpSources" ? mcpSourcesTab() : emptyTab();
  return scenario === "hooks" ? { ...tab, hooks: hooksFor(args.agentId as AgentId) } : tab;
};

let usageHistory: UsageHistoryStatus = {
  state: "kept",
  keptSince: new Date(Date.now() - 90 * 86_400_000).toISOString(),
  foldedThrough: new Date(Date.now() - 7 * 86_400_000).toISOString(),
  bytes: 1_363_148,
};

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
    // A model the rate table has no price for yet: its tokens count, its cost is unknown.
    buckets.push({
      day, provider: "codex", model: "codex-auto-review", totals: totals(900), costUsd: 0,
      cacheSavingsUsd: 0, costSource: "unpriced", records: 12, unpricedRecords: 12, sessions: 2,
    });
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
  list_plugins: catalogTab,
  list_local_plugins: catalogTab,
  refresh: catalogTab,
  read_account_preferences: () => rememberingMock,
  read_accounts: (args) => {
    if (vaultLocked) throw vaultDenied;
    if (scenario === "subscriptionBadges" && args.agent === "codex") return { profiles: subscriptionBadgeProfiles(), nativeObservationId: "badge:renewal", nativeAccount: null, recoveryRequired: false, notice: null };
    if (scenario === "sameEmailWorkspaces" && args.agent === "codex") return {
      profiles: [["personal", "0d6c1f3e-5b2a-4c8e-9f10-2a7b3c4d5e61"], ["business", "7e9a2b4c-1d3f-4a5b-8c6d-9e0f1a2b3c47"]].map(([id, workspaceId], index) => ({
        id, observationId: `profile:${id}`, identity: { provider: "codex", userId: "user-shared", workspaceId },
        label: "shared@example.com", email: "shared@example.com", category: null, savedAt: "2026-09-17T12:00:00Z", active: index === 0, needsLogin: false,
      })),
      nativeObservationId: "profile:personal", nativeAccount: null, recoveryRequired: false, notice: null,
    };
    if (scenario === "limitsOrder" && args.agent === "claude") return { profiles: [], nativeObservationId: "order-current", nativeAccount: null, recoveryRequired: false, notice: null };
    if (scenario === "accountDuplicate" && args.agent === "codex") return {
      profiles: [{ id: "saved", observationId: "profile:shared", identity: { provider: "codex", userId: "shared-user", workspaceId: "ca292064-c3f4-453c-b15a-43ef63c46478" }, label: "shared@example.com", email: "shared@example.com", savedAt: "2026-09-13T12:00:00Z", active: false, needsLogin: false }],
      nativeObservationId: "codex-1", nativeAccount: null, recoveryRequired: false, notice: null,
    };
    return ({
    profiles: [
      { id: "personal", observationId: args.agent === "codex" ? "codex-1" : "claude-1", identity: { provider: args.agent, userId: "user-personal", workspaceId: "Personal workspace" }, label: "personal@example.com", email: "personal@example.com", category: null, savedAt: "2026-08-24T18:30:00Z", active: true, needsLogin: false },
      { id: "work", observationId: args.agent === "codex" ? "codex-2" : "claude-2", identity: { provider: args.agent, userId: "user-work", workspaceId: "Acme workspace" }, label: "person@acme.example", email: "person@acme.example", category: "Client A / research", savedAt: "2026-08-23T14:20:00Z", active: false, needsLogin: false },
    ], nativeObservationId: args.agent === "codex" ? "codex-1" : "claude-1", nativeAccount: { provider: args.agent, userId: "user-personal", workspaceId: "Personal workspace" }, recoveryRequired: false, notice: null,
  }); },
  read_account_activation_blockers: (args) => scenario === "accountClients" && args.agent === "codex" ? ["Acme Studio (codex)", "ChatGPT"] : [],
  account_action: (args) => {
    if (args.action === "unlock") vaultLocked = false;
    if (args.action === "remember") rememberingMock = true;
    if (args.action === "stopRemembering") rememberingMock = false;
  },
  add_account: (args) => scenario === "accountDuplicate"
    ? (duplicateReconnected = true, undefined)
    : scenario === "accountLogin"
    ? new Promise<void>(resolve => pendingLogins.set(String(args.operationId), resolve))
    : undefined,
  cancel_account_login: (args) => {
    const id = String(args.operationId);
    pendingLogins.get(id)?.();
    pendingLogins.delete(id);
  },
  read_limits: (args) => {
    if (scenario === "savedRefreshPaused") return limitsFor(args.agentId).slice(0, 1).flatMap(entry => [entry, {
      ...entry, currentAccount: false, status: "failed", account: {id: `${entry.provider}-saved`, label: "other@example.com"},
      message: "Saved usage access has expired. The last reading is retained.",
    }]);
    if (scenario === "subscriptionBadges" && args.agentId === "codex") return subscriptionBadgeLimits();
    if (scenario === "limitsBand" && args.agentId === "claude") return limitsBandClaude();
    if (scenario === "limitsBand" && args.agentId === "codex") return limitsBandCodex();
    if (scenario === "limitsOrder" && args.agentId === "claude") return limitsOrderClaude();
    if (scenario === "claudeMissingReset" && args.agentId === "claude") return claudeWithoutReset();
    if (scenario === "bankedResets" && args.agentId === "claude") return bankedResetsClaude();
    if (scenario === "bankedResets" && args.agentId === "codex") return bankedResetsCodex();
    if (scenario === "sameEmailWorkspaces" && args.agentId === "codex") return sameEmailWorkspacesCodex();
    if (scenario === "workspaceCredits" && args.agentId === "codex") return workspaceCreditsCodex();
    if (scenario === "creditsSpent" && args.agentId === "codex") return creditsSpentCodex();
    if (scenario === "claudeSubscriptionStatus" && args.agentId === "claude") return claudeSubscriptionStatusClaude();
    const entries = limitsFor(args.agentId);
    if (scenario !== "accountDuplicate" || args.agentId !== "codex") return entries;
    const legacy = { ...entries[1], currentAccount: false, account: {
      id: "ca292064-c3f4-453c-b15a-43ef63c46478", label: "shared@example.com",
    } };
    // Older app versions can rewrite the scoped snapshot without its optional legacyId.
    return [entries[0], ...(duplicateReconnected ? [{ ...legacy,
      account: { id: "profile:shared", label: "shared@example.com" },
      windows: entries[0].windows.map(window => ({ ...window, usedPercent: 42 })),
    }] : []), legacy];
  },
  read_codex_subscription: (args) => scenario === "subscriptionBadges" ? subscriptionBadgeReading(args.accountId) : ({
    metadata: scenario === "subscriptionMissing" ? null : {
      date: "2026-09-24T20:00:00Z",
      kind: args.accountId === "codex-2" ? "paidThrough" : scenario === "subscriptionRenewal" ? "renews" : "expires",
      source: args.accountId === "codex-2" ? "localToken" : "billing",
      checkedAt: "2026-08-24T20:00:00Z",
      stale: args.accountId === "codex-2" || scenario === "subscriptionStale",
    },
    connected: false, browserSupported: true, canConnect: true, unavailable: scenario === "subscriptionStale",
  }),
  consume_codex_reset_credit: () => "reset",
  connect_codex_billing: () => { if (scenario === "billingFailure") throw billingFailure; },
  disconnect_codex_billing: () => undefined,
  usage_summary: (args) => usageSummaryFor(args.input as { sinceDay: string; untilDay: string; timeZone: string }),
  usage_history_status: () => usageHistory,
  clear_usage_history: () => {
    usageHistory = { state: "empty", bytes: 0 };
    return usageHistory;
  },
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
      if (error !== vaultDenied && error !== billingFailure) console.error(`[mock] ${cmd} failed:`, error);
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

let rememberingMock = false;
