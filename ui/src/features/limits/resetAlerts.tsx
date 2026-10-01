import { createContext, useContext, useId, useState } from "react";
import { Segmented } from "@/components/Segmented";
import { SwitchRow } from "@/components/SettingsCard";
import { CODEX_RESET_MAX_LEFT_PERCENT, RESET_ALERT_MAX_HOURS, RESET_AUTO_SPEND_DELAY_MINUTES, defaultResetAlert, resetSpendLimit } from "$lib/appSettings";
import { notificationPermissionProblem } from "$lib/notificationPermission";
import type { ResetAlert } from "$lib/types";

type ResetAlerts = {
  alerts: Record<string, ResetAlert>;
  save: (accountId: string, alert: ResetAlert | null) => Promise<void>;
};

export const ResetAlertsContext = createContext<ResetAlerts | null>(null);

export function useResetSpendLimit(accountId: string): number {
  return resetSpendLimit(useContext(ResetAlertsContext)?.alerts ?? {}, accountId);
}

const button = "rounded-md border border-[var(--hair)] px-2.5 py-1 text-[12px] hover:bg-[var(--wash)] disabled:opacity-50";
const field = "w-16 rounded border border-[var(--hair)] bg-transparent px-2 py-1 text-[12px] tabular-nums";

export function ResetAlertForm({ accountId, label, onDone }: {
  accountId: string;
  label: string;
  onDone: () => void;
}) {
  const context = useContext(ResetAlertsContext);
  if (!context) throw new Error("ResetAlertForm renders inside the Limits screen, which holds the alerts.");
  const { alerts, save } = context;
  const existing = alerts[accountId];
  const [enabled, setEnabled] = useState(existing !== undefined);
  const start = existing ?? defaultResetAlert(label);
  const [left, setLeft] = useState(String(start.maxLeftPercent));
  const [hours, setHours] = useState(String(start.minHoursToRenewal));
  const [automatic, setAutomatic] = useState(start.automatic);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const leftId = useId();
  const hoursId = useId();
  const modeId = useId();
  const leftValue = Number(left);
  const hoursValue = Number(hours);
  const valid = Number.isInteger(leftValue) && leftValue >= 1 && leftValue <= CODEX_RESET_MAX_LEFT_PERCENT
    && Number.isInteger(hoursValue) && hoursValue >= 0 && hoursValue <= RESET_ALERT_MAX_HOURS;

  async function submit() {
    setBusy(true);
    setProblem(null);
    try {
      const denied = enabled ? await notificationPermissionProblem() : null;
      if (denied) {
        setProblem(denied);
        return;
      }
      await save(accountId, enabled ? { label, maxLeftPercent: leftValue, minHoursToRenewal: hoursValue, automatic } : null);
      onDone();
    } catch {
      setProblem("Could not save the alert.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form role="group" aria-label="Banked reset alert" className="flex flex-col gap-2 text-[12px]"
      onSubmit={event => { event.preventDefault(); if (!enabled || valid) void submit(); }}>
      <SwitchRow label="Tell me when this account's banked reset is worth using" on={enabled} onToggle={() => setEnabled(!enabled)} />
      <div className="flex items-center gap-2">
        <span id={modeId} className="min-w-0 flex-1">When it's worth using</span>
        <Segmented
          size="row"
          ariaLabelledBy={modeId}
          options={[
            { value: "notify", label: "Notify me" },
            { value: "automatic", label: "Use it" },
          ]}
          pressed={(mode) => (mode === "automatic") === automatic}
          onPress={(mode) => setAutomatic(mode === "automatic")}
          disabled={!enabled}
        />
      </div>
      <div className="flex items-center gap-2">
        <label htmlFor={leftId} className="min-w-0 flex-1">With this much of the limit left or less (%)</label>
        <input id={leftId} type="number" inputMode="numeric" min={1} max={CODEX_RESET_MAX_LEFT_PERCENT} step={1}
          disabled={!enabled} value={left} onChange={event => setLeft(event.target.value)} className={field} />
      </div>
      <div className="flex items-center gap-2">
        <label htmlFor={hoursId} className="min-w-0 flex-1">And at least this many hours before it renews by itself</label>
        <input id={hoursId} type="number" inputMode="numeric" min={0} max={RESET_ALERT_MAX_HOURS} step={1}
          disabled={!enabled} value={hours} onChange={event => setHours(event.target.value)} className={field} />
      </div>
      <p className="m-0 text-[11px] leading-snug text-[var(--mute)]">
        {automatic
          ? `on-n-off tells you, waits ${RESET_AUTO_SPEND_DELAY_MINUTES} minutes, then uses the reset unless you cancel it on this card. It never uses one with more than ${CODEX_RESET_MAX_LEFT_PERCENT}% of the limit left, and at most once a week.`
          : `You use the reset from this card, and only with ${CODEX_RESET_MAX_LEFT_PERCENT}% or less of the limit left, as in Codex's own app.`}
      </p>
      {enabled && !valid ? (
        <p role="alert" className="m-0 text-[11px] text-[var(--trip)]">
          Use 1 to {CODEX_RESET_MAX_LEFT_PERCENT}% left and 0 to {RESET_ALERT_MAX_HOURS} hours.
        </p>
      ) : null}
      {problem ? <p role="alert" className="m-0 text-[11px] text-[var(--trip)]">{problem}</p> : null}
      <div className="flex gap-2">
        <button className={button} disabled={busy || (enabled && !valid)}>Save alert</button>
        <button type="button" className={button} onClick={onDone}>Cancel</button>
      </div>
    </form>
  );
}
