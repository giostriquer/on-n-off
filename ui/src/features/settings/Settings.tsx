import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { FolderOpen, RefreshCw, X } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { Rocker } from "@/features/agents/Rocker";
import { UpdaterSettingsCard } from "@/features/updater/UpdaterSettingsCard";
import { ProviderIcon } from "$lib/ProviderIcon";
import { visibleAgentIds } from "$lib/appSettings";
import * as api from "$lib/api";
import type {
  AgentId,
  AgentInfo,
  AppSettings,
  GithubPollSeconds,
  LimitsPollMinutes,
  ProviderDiagnose,
} from "$lib/types";

type SettingsProps = {
  agents: AgentInfo[];
  settings: AppSettings;
  onToggleVisible: (id: AgentId, hidden: boolean) => void;
  onSaveBinary: (id: AgentId, path: string) => void;
  onAutomaticUpdatesChange: (enabled: boolean) => void;
  onLimitNotificationsChange: (enabled: boolean) => void;
  onLimitsPollMinutesChange: (minutes: LimitsPollMinutes) => void;
  /** One callback for the Pull requests card; the route merges the patch into the settings. */
  onGithubChange: (patch: GithubSettingsPatch) => void;
};

export type GithubSettingsPatch = Partial<
  Pick<AppSettings, "githubScopes" | "githubNotifications" | "githubPollSeconds">
>;

const GITHUB_POLL_OPTIONS: [GithubPollSeconds, string][] = [
  [30, "30 seconds"],
  [60, "1 minute"],
  [120, "2 minutes"],
  [300, "5 minutes"],
];

const BINARY_NAME: Record<AgentId, string> = {
  claude: "claude",
  codex: "codex",
  antigravity: "agy",
  cursor: "agent",
};

export function Settings({
  agents,
  settings,
  onToggleVisible,
  onSaveBinary,
  onAutomaticUpdatesChange,
  onLimitNotificationsChange,
  onLimitsPollMinutesChange,
  onGithubChange,
}: SettingsProps) {
  const diagnose = useQuery({
    queryKey: ["diagnose-providers", settings.binaryPaths],
    queryFn: () => api.diagnoseProviders(),
  });
  const reports = diagnose.data ?? [];
  const visible = visibleAgentIds(settings.hiddenAgents);

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]">
      <header className="flex flex-wrap items-end gap-3">
        <div>
          <h2 className="m-0 text-[17px] font-semibold tracking-[0.05em] uppercase">Settings</h2>
          <p className="mt-1 font-mono text-[12px] text-[var(--mute)]">
            providers on this machine · hide from tabs · diagnose CLI setup
          </p>
        </div>
        <div className="flex-1" />
        <button
          type="button"
          className="inline-flex size-8 items-center justify-center rounded-lg border border-[var(--hair)] bg-[var(--well)] text-[var(--silkscreen)] disabled:opacity-45"
          aria-label="Re-run diagnose"
          disabled={diagnose.isFetching}
          onClick={() => void diagnose.refetch()}
        >
          <RefreshCw className={`size-3.5 ${diagnose.isFetching ? "animate-spin" : ""}`} aria-hidden="true" />
        </button>
      </header>

      <UpdaterSettingsCard
        automaticUpdates={settings.automaticUpdates}
        onAutomaticUpdatesChange={onAutomaticUpdatesChange}
      />

      <LimitNotificationsCard
        enabled={settings.limitNotifications}
        pollMinutes={settings.limitsPollMinutes}
        onEnabledChange={onLimitNotificationsChange}
        onPollMinutesChange={onLimitsPollMinutesChange}
      />

      <GithubSettingsCard
        scopes={settings.githubScopes}
        enabled={settings.githubNotifications}
        pollSeconds={settings.githubPollSeconds}
        onChange={onGithubChange}
      />

      <section aria-label="Providers">
        <div className="flex flex-col gap-3">
          {agents.map((agent) => (
            <ProviderCard
              key={agent.id}
              agent={agent}
              shown={visible.includes(agent.id)}
              lastVisible={visible.length === 1 && visible[0] === agent.id}
              binaryPath={settings.binaryPaths[agent.id] ?? ""}
              report={reports.find((item) => item.agentId === agent.id) ?? null}
              onToggleVisible={onToggleVisible}
              onSaveBinary={onSaveBinary}
            />
          ))}
        </div>
      </section>
    </div>
  );
}

/**
 * A notifications toggle that asks the OS for permission before turning on. Turning off never
 * asks; a denied or failed request leaves the setting off and explains why.
 */
function useNotificationGate(enabled: boolean, onEnabledChange: (enabled: boolean) => void) {
  const [requestingPermission, setRequestingPermission] = useState(false);
  const [permissionMessage, setPermissionMessage] = useState<string | null>(null);

  async function toggle() {
    if (enabled) {
      setPermissionMessage(null);
      onEnabledChange(false);
      return;
    }
    setRequestingPermission(true);
    setPermissionMessage(null);
    try {
      if (await api.requestNotificationPermission()) {
        onEnabledChange(true);
      } else {
        setPermissionMessage("Notifications are blocked in system settings.");
      }
    } catch {
      setPermissionMessage("Could not request notification permission.");
    } finally {
      setRequestingPermission(false);
    }
  }

  return { requestingPermission, permissionMessage, toggle };
}

function GithubSettingsCard({
  scopes,
  enabled,
  pollSeconds,
  onChange,
}: {
  scopes: string[];
  enabled: boolean;
  pollSeconds: GithubPollSeconds;
  onChange: (patch: GithubSettingsPatch) => void;
}) {
  const { requestingPermission, permissionMessage, toggle } = useNotificationGate(enabled, (value) =>
    onChange({ githubNotifications: value }),
  );
  const [draft, setDraft] = useState("");

  // Enter commits; leaving the field keeps the draft. A blur-commit would race the chip's
  // Remove click: both would persist from the same stale `scopes`, and one write would win.
  function addScope() {
    const scope = draft.trim();
    if (!scope) return;
    setDraft("");
    if (!scopes.includes(scope)) {
      onChange({ githubScopes: [...scopes, scope] });
    }
  }

  return (
    <section aria-label="Pull requests" className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
      <div className="flex flex-wrap items-start gap-3 px-3.5 py-3">
        <div className="min-w-0 flex-1">
          <h3 className="m-0 text-[15px] font-semibold">Pull requests</h3>
          <p className="mt-1 mb-0 text-[11.5px] text-[var(--mute)]">
            Reads GitHub through the `gh` CLI's login. Scopes narrow the pull requests you authored:
            org:NAME, user:NAME or OWNER/REPO. Same-kind scopes combine; mixing kinds narrows.
          </p>
        </div>
        <div className="flex flex-col items-end gap-1">
          <span className="text-[9.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
            CI notify
          </span>
          <Rocker
            size="skill"
            on={enabled}
            busy={requestingPermission}
            ariaLabel="Notify about CI changes"
            onToggle={() => void toggle()}
          />
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2 border-t border-[var(--hair)] px-3.5 py-2.5">
        {scopes.map((scope) => (
          <span
            key={scope}
            className="inline-flex items-center gap-1 rounded-md border border-[var(--hair)] py-0.5 pr-1 pl-1.5 font-mono text-[10.5px]"
          >
            {scope}
            <button
              type="button"
              className="inline-flex size-4 items-center justify-center rounded-sm border-0 bg-transparent p-0 text-[var(--mute)] hover:text-[var(--trip)]"
              aria-label={`Remove ${scope}`}
              onClick={() => onChange({ githubScopes: scopes.filter((item) => item !== scope) })}
            >
              <X className="size-3" aria-hidden="true" />
            </button>
          </span>
        ))}
        <input
          className="h-8 min-w-[12rem] flex-1 rounded-md border border-[var(--hair)] bg-[var(--well)] px-2 font-mono text-[11px] text-[var(--silkscreen)] placeholder:text-[var(--mute)]"
          aria-label="Add a GitHub scope"
          placeholder={scopes.length ? "add another scope, then Enter" : "org:acme · owner/repo · Enter adds · empty means all repositories"}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              addScope();
            }
          }}
        />
      </div>
      <div className="flex flex-wrap items-center gap-3 border-t border-[var(--hair)] px-3.5 py-2.5">
        <label htmlFor="github-poll-seconds" className="min-w-0 flex-1 text-[12px] text-[var(--mute)]">
          Refresh pull requests every
        </label>
        <select
          id="github-poll-seconds"
          aria-label="GitHub polling interval"
          className="h-8 rounded-md border border-[var(--hair)] bg-[var(--well)] px-2 text-[11px] font-semibold"
          value={pollSeconds}
          onChange={(event) => onChange({ githubPollSeconds: Number(event.target.value) as GithubPollSeconds })}
        >
          {GITHUB_POLL_OPTIONS.map(([seconds, label]) => (
            <option key={seconds} value={seconds}>
              {label}
            </option>
          ))}
        </select>
        {permissionMessage ? (
          <p className="m-0 basis-full text-[11.5px] text-[var(--trip)]" role="status">
            {permissionMessage}
          </p>
        ) : null}
      </div>
    </section>
  );
}

function LimitNotificationsCard({
  enabled,
  pollMinutes,
  onEnabledChange,
  onPollMinutesChange,
}: {
  enabled: boolean;
  pollMinutes: LimitsPollMinutes;
  onEnabledChange: (enabled: boolean) => void;
  onPollMinutesChange: (minutes: LimitsPollMinutes) => void;
}) {
  const { requestingPermission, permissionMessage, toggle } = useNotificationGate(enabled, onEnabledChange);

  return (
    <section
      aria-label="Limit notifications"
      className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
    >
      <div className="flex flex-wrap items-start gap-3 px-3.5 py-3">
        <div className="min-w-0 flex-1">
          <h3 className="m-0 text-[15px] font-semibold">Limit notifications</h3>
          <p className="mt-1 mb-0 text-[11.5px] text-[var(--mute)]">
            Notifies when usage reaches 100% or a limit resets while on-n-off is running.
          </p>
        </div>
        <div className="flex flex-col items-end gap-1">
          <span className="text-[9.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
            Notify
          </span>
          <Rocker
            size="skill"
            on={enabled}
            busy={requestingPermission}
            ariaLabel="Notify about limit changes"
            onToggle={() => void toggle()}
          />
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-3 border-t border-[var(--hair)] px-3.5 py-2.5">
        <label htmlFor="limits-poll-minutes" className="min-w-0 flex-1 text-[12px] text-[var(--mute)]">
          Check for limit changes every
        </label>
        <select
          id="limits-poll-minutes"
          aria-label="Limits polling interval"
          className="h-8 rounded-md border border-[var(--hair)] bg-[var(--well)] px-2 text-[11px] font-semibold"
          value={pollMinutes}
          onChange={(event) => onPollMinutesChange(Number(event.target.value) as LimitsPollMinutes)}
        >
          {[5, 10, 15, 30].map((minutes) => (
            <option key={minutes} value={minutes}>
              {minutes} minutes
            </option>
          ))}
        </select>
        {permissionMessage ? (
          <p className="m-0 basis-full text-[11.5px] text-[var(--trip)]" role="status">
            {permissionMessage}
          </p>
        ) : null}
      </div>
    </section>
  );
}

function ProviderCard({
  agent,
  shown,
  lastVisible,
  binaryPath,
  report,
  onToggleVisible,
  onSaveBinary,
}: {
  agent: AgentInfo;
  shown: boolean;
  lastVisible: boolean;
  binaryPath: string;
  report: ProviderDiagnose | null;
  onToggleVisible: (id: AgentId, hidden: boolean) => void;
  onSaveBinary: (id: AgentId, path: string) => void;
}) {
  const [draft, setDraft] = useState(binaryPath);
  const [openDiagnose, setOpenDiagnose] = useState(!agent.cliOk);
  const cliOk = report ? report.checks.some((check) => check.id === "cli" && check.ok) : agent.cliOk;

  useEffect(() => {
    setDraft(binaryPath);
  }, [binaryPath]);

  async function pickBinary() {
    const picked = await open({
      multiple: false,
      filters: [{ name: "CLI", extensions: ["exe", "cmd", "bat"] }],
    });
    if (typeof picked !== "string") {
      return;
    }
    setDraft(picked);
    onSaveBinary(agent.id, picked);
  }

  return (
    <article className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
      <div className="flex flex-wrap items-start gap-3 px-3.5 py-3">
        <ProviderIcon provider={agent.id} className="mt-0.5 size-5 shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="text-[15px] font-semibold">{agent.displayName}</div>
          <div className="mt-1 font-mono text-[11.5px] text-[var(--mute)]">
            {cliOk ? "CLI found" : "CLI missing"}
            {" · "}
            {report?.homePath ?? BINARY_NAME[agent.id]}
          </div>
        </div>
        <span
          className={`mt-0.5 shrink-0 border px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] ${
            cliOk ? "border-[var(--live)] text-[var(--live)]" : "border-[var(--trip)] text-[var(--trip)]"
          }`}
        >
          {cliOk ? "OK" : "DOWN"}
        </span>
        <div className="flex flex-col items-end gap-1">
          <span className="text-[9.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
            Show in tabs
          </span>
          <Rocker
            size="skill"
            on={shown}
            disabled={lastVisible && shown}
            ariaLabel={`Show ${agent.displayName} in agent tabs`}
            onToggle={() => onToggleVisible(agent.id, shown)}
          />
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2 border-t border-[var(--hair)] px-3.5 py-2.5">
        <span className="w-[88px] shrink-0 text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">
          Binary
        </span>
        <input
          className="min-w-0 flex-1 rounded-md border border-[var(--hair)] bg-[var(--well)] px-2 py-1 font-mono text-[12px] text-[var(--silkscreen)] placeholder:text-[var(--mute)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--fill)]"
          value={draft}
          placeholder={BINARY_NAME[agent.id]}
          spellCheck={false}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={() => {
            if (draft.trim() !== binaryPath.trim()) {
              onSaveBinary(agent.id, draft);
            }
          }}
        />
        <button
          type="button"
          className="inline-flex size-8 items-center justify-center rounded-md border border-[var(--hair)] bg-[var(--well)]"
          aria-label={`Browse ${agent.displayName} CLI`}
          onClick={() => void pickBinary()}
        >
          <FolderOpen className="size-3.5" aria-hidden="true" />
        </button>
        <button
          type="button"
          className="h-8 rounded-md border border-[var(--hair)] px-2.5 text-[10px] font-semibold tracking-[0.04em] uppercase"
          aria-expanded={openDiagnose}
          onClick={() => setOpenDiagnose((open) => !open)}
        >
          Diagnose
        </button>
      </div>

      {openDiagnose ? (
        <div className="border-t border-[var(--hair)] px-3.5 py-2.5">
          {(report?.checks ?? []).length === 0 ? (
            <p className="text-[13px] text-[var(--mute)]">Scanning this machine…</p>
          ) : (
            <ul className="m-0 flex list-none flex-col gap-2 p-0">
              {report?.checks.map((check) => (
                <li key={check.id} className="flex items-start gap-2.5">
                  <span
                    className={`mt-0.5 size-2 shrink-0 rounded-full ${
                      check.ok ? "bg-[var(--live)] shadow-[0_0_7px_var(--live)]" : "bg-[var(--trip)]"
                    }`}
                    aria-hidden="true"
                  />
                  <div className="min-w-0">
                    <div className="text-[12.5px] font-semibold">{check.label}</div>
                    <div className="font-mono text-[11px] leading-snug text-[var(--mute)]">{check.detail}</div>
                    {check.hint ? (
                      <div className="mt-0.5 text-[11.5px] leading-snug text-[var(--mute)]">{check.hint}</div>
                    ) : null}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : null}
    </article>
  );
}
