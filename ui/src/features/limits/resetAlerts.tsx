import { createContext, useContext, useId, useState } from "react";
import { CODEX_RESET_MAX_LEFT_PERCENT, RESET_ALERT_MAX_HOURS, defaultResetAlert, resetSpendLimit } from "$lib/appSettings";
import { notificationPermissionProblem } from "$lib/notificationPermission";
import type { ResetAlert } from "$lib/types";

/**
 * The banked reset alerts the Limits screen was given, by the card's account id, and how to change
 * one: the app's settings, saved through the session so no screen writes a stale copy over another.
 */
type ResetAlerts = {
  alerts: Record<string, ResetAlert>;
  save: (accountId: string, alert: ResetAlert | null) => Promise<void>;
};

export const ResetAlertsContext = createContext<ResetAlerts | null>(null);

/**
 * The share of the current limit left at or under which `accountId`'s banked reset may be spent
 * (`resetSpendLimit`): Codex's own 10% outside the Limits screen, which holds no alerts.
 */
export function useResetSpendLimit(accountId: string): number {
  return resetSpendLimit(useContext(ResetAlertsContext)?.alerts ?? {}, accountId);
}

const button = "rounded-md border border-[var(--hair)] px-2.5 py-1 text-[12px] hover:bg-[var(--wash)] disabled:opacity-50";
const field = "w-16 rounded border border-[var(--hair)] bg-transparent px-2 py-1 text-[12px] tabular-nums";

/**
 * A Codex account's banked reset alert, as its card's menu edits it: whether on-n-off offers the
 * account's reset once it runs low, at how much left, and how long before its limit renews by
 * itself. The reset is still spent from the card, as in Codex's own app.
 */
export function ResetAlertForm({ accountId, label, onDone }: {
  accountId: string;
  /** The account's email as the card shows it, kept with the alert for Settings to name it. */
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
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const leftId = useId();
  const hoursId = useId();
  const leftValue = Number(left);
  const hoursValue = Number(hours);
  const valid = Number.isInteger(leftValue) && leftValue >= 1 && leftValue <= CODEX_RESET_MAX_LEFT_PERCENT
    && Number.isInteger(hoursValue) && hoursValue >= 0 && hoursValue <= RESET_ALERT_MAX_HOURS;

  async function submit() {
    setBusy(true);
    setProblem(null);
    try {
      // The alert is a notification: without permission to show one, it would never be seen.
      const denied = enabled ? await notificationPermissionProblem() : null;
      if (denied) {
        setProblem(denied);
        return;
      }
      await save(accountId, enabled ? { label, maxLeftPercent: leftValue, minHoursToRenewal: hoursValue } : null);
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
      <label className="flex items-start gap-2">
        <input type="checkbox" checked={enabled} onChange={event => setEnabled(event.target.checked)} className="mt-0.5" />
        <span>Tell me when this account's banked reset is worth using</span>
      </label>
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
        on-n-off never uses a reset by itself: you use it from this card, and only with {CODEX_RESET_MAX_LEFT_PERCENT}% or less of the limit left, as in Codex's own app.
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
