import { useRef, useState } from "react";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { formatResetIn, formatShortDate } from "$lib/limitsFormat";
import type { LimitsResetCredits, ProviderLimits, ResetCreditOutcome } from "$lib/limitsTypes";
import { accountButton } from "@/features/accounts/AccountManager";
import { ConfirmDialog } from "@/features/catalog/ConfirmDialog";
import { usageLeft } from "./limitPresentation";
import { SummaryRow } from "./SummaryRow";

/** Below this much usage left a banked reset is doing what it is for, so it is spent without asking. */
const SPEND_WITHOUT_ASKING_BELOW = 5;

const OUTCOME_MESSAGES: Record<ResetCreditOutcome, string> = {
  reset: "Banked reset used.",
  nothingToReset: "Nothing to reset: this account's usage is already at 0%.",
  noCredit: "This account has no banked reset left.",
  alreadyRedeemed: "That banked reset was already used.",
  unknown: "Codex answered with a result on-n-off doesn't recognize. Check the reset count after the refresh.",
};

/** The banked reset count as one more row under the windows, with when the next one expires. */
export function BankedResetsRow({ resetCredits, now }: { resetCredits?: LimitsResetCredits | null; now: number }) {
  if (!resetCredits || resetCredits.availableCount <= 0) return null;
  const expiresIn = formatResetIn(resetCredits.nextExpiresAt, now);
  const lead = resetCredits.availableCount > 1 ? "next expires" : "expires";
  const note = expiresIn ? `${lead} in ${expiresIn} · ${formatShortDate(resetCredits.nextExpiresAt)}` : undefined;
  return <SummaryRow label="Banked resets" value={resetCredits.availableCount} note={note} />;
}

/**
 * Spends one banked reset on the signed-in Codex account. Codex applies a reset to whoever is signed
 * in, so only the card that is both the live read and the account controls' current account offers
 * it. With less than 5% of usage left it spends straight away; otherwise it asks first, naming the
 * account and what is left, because a reset used early is a reset wasted.
 *
 * One attempt keeps one idempotency key until Codex gives a definite answer: a retry after an error
 * may be retrying a request that already went through, and a new key would spend a second reset.
 * The refreshed limits arrive through the shared read the backend replaces after every attempt.
 */
export function UseBankedReset({ entry, label, current, now, disabled = false }: {
  entry: ProviderLimits;
  /** The name the card shows for this account, so the confirmation names the same one. */
  label: string;
  current: boolean;
  now: number;
  disabled?: boolean;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ role: "status" | "alert"; message: string } | null>(null);
  const attempt = useRef<string | null>(null);
  const accountId = entry.account?.id;
  const offered = current && entry.currentAccount && entry.status === "ok" && (entry.resetCredits?.availableCount ?? 0) > 0;
  if (!accountId || (!offered && !result)) return null;
  const left = usageLeft(entry, now);

  async function spend(account: string) {
    setConfirming(false);
    setBusy(true);
    setResult(null);
    attempt.current ??= crypto.randomUUID();
    try {
      const outcome = await api.consumeCodexResetCredit(account, attempt.current);
      attempt.current = null;
      setResult({ role: "status", message: OUTCOME_MESSAGES[outcome] });
    } catch (error) {
      setResult({ role: "alert", message: parseInvokeError(error).message });
    } finally {
      setBusy(false);
    }
  }

  function request(account: string) {
    if (busy) return;
    if (left !== null && left < SPEND_WITHOUT_ASKING_BELOW) void spend(account);
    else setConfirming(true);
  }

  // A non-breaking hyphen keeps "5-hour" on one line in the dialog.
  const effect = "A banked reset puts the 5\u2011hour and weekly windows back to 0% and moves your weekly reset date. It can't be undone.";
  const body = left === null
    ? `on-n-off can't tell how much Codex usage ${label} has left. ${effect}`
    : `${label} still has ${Math.round(left)}% of its Codex usage left, so a reset is worth more once you run out. ${effect}`;
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
