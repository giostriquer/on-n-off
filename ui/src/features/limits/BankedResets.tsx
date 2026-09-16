import { useId, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { formatResetIn, formatShortDate } from "$lib/limitsFormat";
import type { LimitsResetCredits, ProviderLimits, ResetCreditOutcome } from "$lib/limitsTypes";
import { accountButton } from "@/features/accounts/AccountManager";
import { ConfirmDialog } from "@/features/catalog/ConfirmDialog";
import { usageLeft } from "./limitPresentation";

/** Below this much usage left a banked reset is doing what it is for, so it is spent without asking. */
const SPEND_WITHOUT_ASKING_BELOW = 5;

const OUTCOME_MESSAGES: Record<ResetCreditOutcome, string> = {
  reset: "Banked reset used.",
  nothingToReset: "Nothing to reset: this account's usage is already at 0%.",
  noCredit: "This account has no banked reset left.",
  alreadyRedeemed: "That banked reset was already used.",
};

/** The banked reset count as one more row under the windows, with when the next one expires. */
export function BankedResetsRow({ resetCredits, now }: { resetCredits?: LimitsResetCredits | null; now: number }) {
  const labelId = useId();
  if (!resetCredits || resetCredits.availableCount <= 0) return null;
  const expiresIn = formatResetIn(resetCredits.nextExpiresAt, now);
  const lead = resetCredits.availableCount > 1 ? "next expires" : "expires";
  const note = expiresIn ? `${lead} in ${expiresIn} · ${formatShortDate(resetCredits.nextExpiresAt)}` : "";
  return (
    <dl className="flex items-center gap-2.5 border-t border-[var(--hair)] px-3.5 py-2">
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <dt id={labelId} className="text-[10px] leading-4 font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">
          Banked resets
        </dt>
        {note ? <dd className="font-mono text-[11px] leading-snug text-[var(--mute)]">{note}</dd> : null}
      </div>
      <dd aria-labelledby={labelId} className="shrink-0 text-right font-mono text-[12px] tabular-nums">
        {resetCredits.availableCount}
      </dd>
    </dl>
  );
}

/**
 * Spends one banked reset on the signed-in Codex account. Codex applies a reset to whoever is signed
 * in, so only the live current card offers it. With less than 5% of usage left it spends straight
 * away; otherwise it asks first, naming what is left, because a reset used early is a reset wasted.
 */
export function UseBankedReset({ entry, label, now, disabled = false }: {
  entry: ProviderLimits;
  /** The name the card shows for this account, so the confirmation names the same one. */
  label?: string | null;
  now: number;
  disabled?: boolean;
}) {
  const client = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ role: "status" | "alert"; message: string } | null>(null);
  const accountId = entry.account?.id;
  const available = entry.resetCredits?.availableCount ?? 0;
  const offered = entry.provider === "codex" && entry.currentAccount && entry.status === "ok" && !!accountId && available > 0;
  if (!accountId || (!offered && !result)) return null;
  const left = usageLeft(entry, now);

  async function spend(account: string) {
    setConfirming(false);
    setBusy(true);
    setResult(null);
    try {
      const outcome = await api.consumeCodexResetCredit(account, crypto.randomUUID());
      setResult({ role: "status", message: OUTCOME_MESSAGES[outcome] });
    } catch (error) {
      setResult({ role: "alert", message: parseInvokeError(error).message });
    } finally {
      setBusy(false);
      await client.invalidateQueries({ queryKey: ["limits", "codex"] });
    }
  }

  function request(account: string) {
    if (left !== null && left < SPEND_WITHOUT_ASKING_BELOW) void spend(account);
    else setConfirming(true);
  }

  // A non-breaking hyphen keeps "5-hour" on one line in the dialog.
  const effect = "A banked reset puts the 5\u2011hour and weekly windows back to 0% and moves your weekly reset date. It can't be undone.";
  const name = label || entry.account?.label;
  const body = left === null
    ? `on-n-off can't tell how much Codex usage ${name || "this account"} has left. ${effect}`
    : `${name || "This account"} still has ${Math.round(left)}% of its Codex usage left, so a reset is worth more once you run out. ${effect}`;
  return (
    <>
      {offered ? (
        <button type="button" className={accountButton} disabled={disabled || busy} onClick={() => request(accountId)}>
          {busy ? "Using reset…" : "Use banked reset"}
        </button>
      ) : null}
      {result ? (
        <p role={result.role} className={`m-0 min-w-0 basis-full break-words text-[11px] ${result.role === "alert" ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}>
          {result.message}
        </p>
      ) : null}
      {confirming ? (
        <ConfirmDialog
          title="Use a banked reset now?"
          body={body}
          confirmLabel="Use reset anyway"
          busy={busy}
          onCancel={() => setConfirming(false)}
          onConfirm={() => void spend(accountId)}
        />
      ) : null}
    </>
  );
}
