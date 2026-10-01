import { useRef, useState } from "react";
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

export function ResetOfferRow({ offer }: { offer?: LimitsResetOffer | null }) {
  if (!offer) return null;
  return <SummaryRow label="Paid reset" value={offer.price ? formatPrice(offer.price) : "offered"} note="offered by Codex · buy it on chatgpt.com" />;
}

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

function describeReset(reset: LimitsBankedReset, now: number): string {
  const when = expiry(reset.expiresAt, now);
  return [reset.title, when && `expires ${when}`].filter(Boolean).join(" · ") || "Banked reset";
}

function expiry(at: string | null | undefined, now: number): string | null {
  const left = formatResetIn(at, now);
  return left ? `in ${left} · ${formatShortDate(at)}` : null;
}

type AttemptResult = { role: "status" | "alert"; message: string; answeredAt: number };

export function UseBankedReset({ entry, label, current, now, disabled = false }: {
  entry: ProviderLimits;
  label: string;
  current: boolean;
  now: number;
  disabled?: boolean;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<AttemptResult | null>(null);
  const attempt = useRef<string | null>(null);
  const accountId = entry.account?.id;
  const limit = useResetSpendLimit(accountId ?? "");
  const offered = current && entry.currentAccount && entry.status === "ok" && unexpiredBankedResets(entry.resetCredits, now) !== null;
  const observed = latestObservedAt(entry);
  const shown = result && (observed === null || observed <= result.answeredAt) ? result : null;
  if (!accountId || (!offered && !shown)) return null;
  const read = usageLeft(entry, now);
  const left = read === null ? null : Math.round(read);

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
  const effect = "A banked reset puts its usage back to 0% and moves its weekly reset date. It can't be undone.";
  const body = left === null
    ? `on-n-off can't read how much of ${label}'s Codex limit is left.${renewsIn ? ` It renews by itself in ${renewsIn}.` : ""} ${effect}`
    : `${label} has ${left}% of its Codex limit left${renewsIn ? ` and renews by itself in ${renewsIn}` : ""}. ${effect}`;
  return (
    <>
      {offered ? (
        <button type="button" className={accountButton} disabled={disabled || busy}
          onClick={() => { if (!busy) setConfirming(true); }}>
          {busy ? "Using reset…" : "Use banked reset"}
        </button>
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
          warning={spendWarning(left, limit)}
          confirmLabel="Use reset"
          busy={busy}
          onCancel={() => setConfirming(false)}
          onConfirm={() => void spend(accountId)}
        />
      ) : null}
    </>
  );
}

function spendWarning(left: number | null, limit: number): string | null {
  if (left === null) return `More than ${limit}% of the limit may still be left.`;
  if (left <= limit) return null;
  return `More than ${limit}% of the limit is still left: the reset gives back only the ${100 - left}% used so far.`;
}
