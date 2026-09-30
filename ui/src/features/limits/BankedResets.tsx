import { useId, useRef, useState } from "react";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { formatPrice, formatResetIn, formatShortDate } from "$lib/limitsFormat";
import type { LimitsBankedReset, LimitsResetOffer, ProviderLimits, ResetCreditOutcome } from "$lib/limitsTypes";
import { accountButton } from "@/features/accounts/AccountManager";
import { ConfirmDialog } from "@/features/catalog/ConfirmDialog";
import type { CardFigures } from "./limitCards";
import { latestObservedAt, unexpiredBankedResets, usageLeft } from "./limitPresentation";
import { useResetSpendLimit } from "./resetAlerts";
import { SummaryRow } from "./SummaryRow";

const OUTCOME_MESSAGES: Record<ResetCreditOutcome, string> = {
  reset: "Banked reset used.",
  nothingToReset: "Nothing to reset: this account's usage is already at 0%.",
  noCredit: "This account has no banked reset left.",
  alreadyRedeemed: "That banked reset was already used.",
  unknown: "Codex answered with a result on-n-off doesn't recognize. Check the reset count after the refresh.",
};

/**
 * A paid reset the provider is offering while the account sits at its limit. It is shown, never
 * sold: the purchase happens on the provider's own site, so this row carries no action.
 */
export function ResetOfferRow({ offer }: { offer?: LimitsResetOffer | null }) {
  if (!offer) return null;
  return <SummaryRow label="Paid reset" value={offer.price ? formatPrice(offer.price) : "offered"} note="offered by Codex · buy it on chatgpt.com" />;
}

/**
 * The banked reset count as one more row under the windows, with when the next one expires. When
 * Codex lists more than one, the note
 * lists each instead, soonest first, by name and expiry, so its first line says what the
 * next-expiry note would. Whether a count is worth showing is the card model's call (`CardFigures`),
 * and it is gone by the time the soonest reset lapses, so every one listed is still ahead.
 */
export function BankedResetsRow({ resetCredits, now }: { resetCredits: CardFigures["bankedResets"]; now: number }) {
  if (!resetCredits) return null;
  const resets = resetCredits.resets ?? [];
  const lead = resetCredits.availableCount > 1 ? "next expires" : "expires";
  const next = expiry(resetCredits.nextExpiresAt, now);
  const note = resets.length > 1
    ? { label: "Each banked reset", lines: resets.map(reset => describeReset(reset, now)) }
    : next ? `${lead} ${next}` : undefined;
  return <SummaryRow label="Banked resets" value={resetCredits.availableCount} note={note} />;
}

/** One banked reset's line: its name and when it expires, each when Codex says. */
function describeReset(reset: LimitsBankedReset, now: number): string {
  const when = expiry(reset.expiresAt, now);
  return [reset.title, when && `expires ${when}`].filter(Boolean).join(" · ") || "Banked reset";
}

/** "in 11d 19h · Sep 5" for an instant still ahead; `null` for one that is not. */
function expiry(at: string | null | undefined, now: number): string | null {
  const left = formatResetIn(at, now);
  return left ? `in ${left} · ${formatShortDate(at)}` : null;
}

/** What one attempt to spend a banked reset came to, and when its answer arrived. */
type AttemptResult = { role: "status" | "alert"; message: string; answeredAt: number };

/**
 * Spends one banked reset on the signed-in Codex account. Codex applies a reset to whoever is signed
 * in, so only the card that is both the live read and the account controls' current account offers
 * it. As in Codex's own app, it is usable only with 10% or less of the limit left (or the lower share
 * the account's alert names), and every use asks first, naming the account, what is left and when
 * the limit renews by itself, because a reset used early is a reset wasted. The backend refuses a
 * spend above that share too.
 *
 * One attempt keeps one idempotency key until Codex gives a definite answer: a retry after an error
 * may be retrying a request that already went through, and a new key would spend a second reset.
 * The refreshed limits arrive through the shared read the backend replaces after every attempt.
 *
 * What an attempt came to is said until a reading made after its answer replaces the card's: the
 * backend's refresh is read before the answer comes back, so it keeps the message, and the next
 * read, which may find a reset granted since, lets it go. The answer is timed by the clock the
 * backend stamps its readings with, not the screen's `now`, which lags it by up to a minute and
 * would let the refresh take the message away. A card with no window read at all has nothing to
 * tell a later reading by, so it keeps the message.
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
  const [result, setResult] = useState<AttemptResult | null>(null);
  const attempt = useRef<string | null>(null);
  const ruleId = useId();
  const accountId = entry.account?.id;
  const limit = useResetSpendLimit(accountId ?? "");
  const offered = current && entry.currentAccount && entry.status === "ok" && unexpiredBankedResets(entry.resetCredits, now) !== null;
  const observed = latestObservedAt(entry);
  const shown = result && (observed === null || observed <= result.answeredAt) ? result : null;
  if (!accountId || (!offered && !shown)) return null;
  const left = usageLeft(entry, now);
  const allowed = left !== null && left <= limit;

  async function spend(account: string) {
    setConfirming(false);
    setBusy(true);
    setResult(null);
    attempt.current ??= crypto.randomUUID();
    try {
      const outcome = await api.consumeCodexResetCredit(account, attempt.current);
      attempt.current = null;
      setResult({ role: "status", message: OUTCOME_MESSAGES[outcome], answeredAt: Date.now() });
    } catch (error) {
      setResult({ role: "alert", message: parseInvokeError(error).message, answeredAt: Date.now() });
    } finally {
      setBusy(false);
    }
  }

  const weekly = entry.windows.find(window => window.kind === "weekly");
  const renewsIn = formatResetIn(weekly?.resetsAt, now);
  const renews = renewsIn ? ` and renews by itself in ${renewsIn}` : "";
  const body = `${label} has ${Math.round(left ?? 0)}% of its Codex limit left${renews}. A banked reset puts its usage back to 0% and moves its weekly reset date. It can't be undone.`;
  return (
    <>
      {offered ? (
        <button type="button" className={accountButton} disabled={disabled || busy || !allowed}
          aria-describedby={allowed ? undefined : ruleId}
          onClick={() => { if (!busy && allowed) setConfirming(true); }}>
          {busy ? "Using reset…" : "Use banked reset"}
        </button>
      ) : null}
      {offered && !allowed ? (
        <span id={ruleId} className="text-[11px] text-[var(--mute)]">
          Usable once {limit}% or less of the limit is left
        </span>
      ) : null}
      {shown ? (
        <p role={shown.role} className={`m-0 min-w-0 basis-full break-words text-[11px] ${shown.role === "alert" ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}>
          {shown.message}
        </p>
      ) : null}
      {confirming ? (
        <ConfirmDialog
          title="Use this reset?"
          body={body}
          confirmLabel="Use reset"
          busy={busy}
          onCancel={() => setConfirming(false)}
          onConfirm={() => void spend(accountId)}
        />
      ) : null}
    </>
  );
}
