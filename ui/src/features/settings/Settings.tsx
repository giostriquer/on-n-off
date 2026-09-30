import { AccountPreferences } from "@/features/accounts/AccountPreferences";
import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { FolderOpen, X } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { CardToggle, SettingRow, SettingsCard, caption, cardButton, cardIconButton, cardSelect, rowLabel } from "@/components/SettingsCard";
import { UpdaterSettingsCard } from "@/features/updater/UpdaterSettingsCard";
import { NotchSettingsCard } from "@/features/notch/NotchSettingsCard";
import { TraySettingsCard } from "./TraySettingsCard";
import { UsageHistoryCard } from "@/features/usage/UsageHistoryCard";
import { FOCUS_RING } from "$lib/a11y";
import { ProviderIcon } from "$lib/ProviderIcon";
import { visibleAgentIds, withResetAlert } from "$lib/appSettings";
import { notificationPermissionProblem } from "$lib/notificationPermission";
import * as api from "$lib/api";
import type {
  AgentId,
  AgentInfo,
  AppSettings,
  GithubPollSeconds,
  LimitsPollMinutes,
  ProviderDiagnose,
  ResetAlert,
} from "$lib/types";

type SettingsProps = {
  agents: AgentInfo[];
  settings: AppSettings;
  onToggleVisible: (id: AgentId, hidden: boolean) => void;
  onSaveBinary: (id: AgentId, path: string) => void;
  onAutomaticUpdatesChange: (enabled: boolean) => void;
  onLimitNotificationsChange: (enabled: boolean) => void;
  onLimitsPollMinutesChange: (minutes: LimitsPollMinutes) => void;
  /** The cards that patch settings directly; the route merges the patch and persists it. */
  onSettingsChange: (patch: Partial<AppSettings>) => void;
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
  onSettingsChange,
}: SettingsProps) {
  // Keyed on the health the shell already probed, not just the binary overrides: a refresh that
  // finds a newly installed CLI would otherwise leave this card listing the failure that fixed.
  const health = agents.map((agent) => `${agent.id}:${agent.cliOk}`).join(",");
  const diagnose = useQuery({
    queryKey: ["diagnose-providers", settings.binaryPaths, health],
    queryFn: () => api.diagnoseProviders(),
  });
  const reports = diagnose.data ?? [];
  const visible = visibleAgentIds(settings.hiddenAgents);

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]">
      <header className="flex flex-wrap items-end gap-3">
        <div>
          <h2 className="m-0 text-[15px] font-semibold tracking-[0.05em] uppercase">Settings</h2>
          <p className="mt-1 font-mono text-[12px] text-[var(--mute)]">
            providers on this machine · hide from tabs · diagnose CLI setup
          </p>
        </div>
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

      <ResetAlertsCard
        alerts={settings.resetAlerts}
        onChange={(resetAlerts) => onSettingsChange({ resetAlerts })}
      />

      <TraySettingsCard
        closeToTray={settings.closeToTray}
        onCloseToTrayChange={(enabled) => onSettingsChange({ closeToTray: enabled })}
      />

      <NotchSettingsCard />
      <AccountPreferences />

      <GithubSettingsCard
        scopes={settings.githubScopes}
        enabled={settings.githubNotifications}
        pollSeconds={settings.githubPollSeconds}
        onChange={onSettingsChange}
      />

      <UsageHistoryCard />

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
    const problem = await notificationPermissionProblem();
    setRequestingPermission(false);
    if (problem) setPermissionMessage(problem);
    else onEnabledChange(true);
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
    <SettingsCard
      title="Pull requests"
      description="Reads GitHub through the `gh` CLI's login; nothing is written to GitHub."
      control={
        <CardToggle
          caption="PR notify"
          on={enabled}
          busy={requestingPermission}
          ariaLabel="Notify about CI, review and merge changes"
          onToggle={() => void toggle()}
        />
      }
    >
      <SettingRow stack>
        <div className="flex flex-col gap-1">
          <label htmlFor="github-scope" className="text-[12px] text-[var(--mute)]">
            Scopes
          </label>
          <span id="github-scope-help" className="font-mono text-[11px] text-[var(--mute)]">
            org:NAME, user:NAME or OWNER/REPO narrow the pull requests you authored · Enter adds · empty
            means all repositories · same-kind scopes combine, mixing kinds narrows
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {scopes.map((scope) => (
            <span
              key={scope}
              className="inline-flex items-center gap-0.5 rounded-md border border-[var(--hair)] py-0.5 pr-0.5 pl-1.5 font-mono text-[11px]"
            >
              {scope}
              <button
                type="button"
                className={`inline-flex size-6 items-center justify-center rounded-sm border-0 bg-transparent p-0 text-[var(--mute)] hover:text-[var(--trip)] ${FOCUS_RING}`}
                aria-label={`Remove ${scope}`}
                onClick={() => onChange({ githubScopes: scopes.filter((item) => item !== scope) })}
              >
                <X className="size-3" aria-hidden="true" />
              </button>
            </span>
          ))}
          <input
            id="github-scope"
            className={`h-8 min-w-[12rem] flex-1 rounded-md border border-[var(--hair)] bg-[var(--well)] px-2 font-mono text-[11px] text-[var(--silkscreen)] placeholder:text-[var(--mute)] ${FOCUS_RING}`}
            aria-describedby="github-scope-help"
            placeholder={scopes.length ? "add another" : "org:acme"}
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
      </SettingRow>
      <SettingRow>
        <label htmlFor="github-poll-seconds" className={rowLabel}>
          Refresh pull requests every
        </label>
        <select
          id="github-poll-seconds"
          aria-label="GitHub polling interval"
          className={cardSelect}
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
          <p className="m-0 basis-full text-[12px] text-[var(--trip)]" role="status">
            {permissionMessage}
          </p>
        ) : null}
      </SettingRow>
    </SettingsCard>
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
    <SettingsCard
      label="Usage refresh and limit notifications"
      title="Usage limits"
      description="Notifies when usage reaches 100% or a limit resets while on-n-off is running."
      control={
        <CardToggle
          caption="Notify"
          on={enabled}
          busy={requestingPermission}
          ariaLabel="Notify about limit changes"
          onToggle={() => void toggle()}
        />
      }
    >
      <SettingRow>
        <label htmlFor="limits-poll-minutes" className={rowLabel}>
          Refresh usage across the app every
        </label>
        <select
          id="limits-poll-minutes"
          aria-label="Limits polling interval"
          className={cardSelect}
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
          <p className="m-0 basis-full text-[12px] text-[var(--trip)]" role="status">
            {permissionMessage}
          </p>
        ) : null}
      </SettingRow>
    </SettingsCard>
  );
}

/**
 * The Codex accounts whose banked reset on-n-off offers once they run low. An alert is turned on
 * from the account's card on Limits, where its label is; here it can be read and turned off.
 */
function ResetAlertsCard({ alerts, onChange }: {
  alerts: Record<string, ResetAlert>;
  onChange: (alerts: Record<string, ResetAlert>) => void;
}) {
  const entries = Object.entries(alerts);
  return (
    <SettingsCard
      title="Banked reset alerts"
      description="Notifies when a Codex account runs low and one of its banked resets is worth using. on-n-off never uses a reset by itself; you use it from the account's card. Turn an alert on from the account's ••• menu on Limits."
    >
      {entries.length > 0 ? (
        <ul aria-label="Accounts with a banked reset alert" className="m-0 list-none p-0">
          {entries.map(([accountId, alert]) => {
            const name = alert.label || "Codex account";
            return (
              <li key={accountId}>
                <SettingRow>
                  <span className={rowLabel}>
                    <span className="font-medium text-[var(--silkscreen)]">{name}</span>
                    {" "}· {alert.maxLeftPercent}% or less left, {alert.minHoursToRenewal}h or more before it renews
                  </span>
                  <button
                    type="button"
                    aria-label={`Turn off the banked reset alert for ${name}`}
                    className={cardButton}
                    onClick={() => onChange(withResetAlert(alerts, accountId, null))}
                  >
                    Turn off
                  </button>
                </SettingRow>
              </li>
            );
          })}
        </ul>
      ) : null}
    </SettingsCard>
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
    <SettingsCard
      as="article"
      title={agent.displayName}
      icon={<ProviderIcon provider={agent.id} className="mt-0.5 size-5 shrink-0" />}
      meta={`${cliOk ? "CLI found" : "CLI missing"} · ${report?.homePath ?? BINARY_NAME[agent.id]}`}
      control={
        <>
          <span
            className={`mt-0.5 shrink-0 border px-1.5 py-0.5 text-[10px] font-semibold tracking-[0.03em] ${
              cliOk ? "border-[var(--live)] text-[var(--live)]" : "border-[var(--trip)] text-[var(--trip)]"
            }`}
          >
            {cliOk ? "OK" : "DOWN"}
          </span>
          <CardToggle
            caption="Show in tabs"
            on={shown}
            disabled={lastVisible && shown}
            ariaLabel={`Show ${agent.displayName} in agent tabs`}
            onToggle={() => onToggleVisible(agent.id, shown)}
          />
        </>
      }
    >
      <SettingRow>
        <span className={`w-[88px] shrink-0 ${caption}`}>
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
          className={cardIconButton}
          aria-label={`Browse ${agent.displayName} CLI`}
          onClick={() => void pickBinary()}
        >
          <FolderOpen className="size-3.5" aria-hidden="true" />
        </button>
        <button
          type="button"
          className={cardButton}
          aria-expanded={openDiagnose}
          onClick={() => setOpenDiagnose((open) => !open)}
        >
          Diagnose
        </button>
      </SettingRow>

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
                    <div className="text-[12px] font-semibold">{check.label}</div>
                    <div className="font-mono text-[11px] leading-snug text-[var(--mute)]">{check.detail}</div>
                    {check.hint ? (
                      <div className="mt-0.5 text-[12px] leading-snug text-[var(--mute)]">{check.hint}</div>
                    ) : null}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : null}
    </SettingsCard>
  );
}
