import { formatShortDate, hasElapsed } from "$lib/limitsFormat";
import type { LimitsWorkspaceCredits, ProviderLimits } from "$lib/limitsTypes";
import { SummaryRow } from "./SummaryRow";

const AMOUNT = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

/**
 * A business member's share of the workspace's pooled credits (Codex's spend control) as one row
 * under the windows: what is left of it, or that it is used up, and when it resets. Once it has
 * reset, what is used is not known until the next read, so the row goes.
 */
export function WorkspaceCreditsRow({ share, now }: { share?: LimitsWorkspaceCredits | null; now: number }) {
  if (!share || (share.resetsAt && hasElapsed(share.resetsAt, now))) return null;
  const limit = Number(share.limit);
  const used = Number(share.used);
  const known = share.limit.trim() !== "" && Number.isFinite(limit) && Number.isFinite(used);
  const value = share.reached
    ? "Used up"
    : known
      ? `${AMOUNT.format(Math.max(limit - used, 0))} of ${AMOUNT.format(limit)} left`
      : `${share.remainingPercent}% left`;
  const note = [
    share.reached && known ? `all ${AMOUNT.format(limit)} used` : undefined,
    share.resetsAt ? `resets ${formatShortDate(share.resetsAt)}` : undefined,
  ]
    .filter(Boolean)
    .join(" · ");
  return <SummaryRow label="Workspace credits" value={value} note={note || undefined} />;
}

/**
 * Whether the member's own credit balance is worth a row. In a business workspace the credits are
 * the workspace's, so the member's own balance reads 0 beside a share that says what they can use.
 */
export function showsOwnCredits(entry: Pick<ProviderLimits, "credits" | "workspaceCredits">): boolean {
  const credits = entry.credits;
  if (!credits) return false;
  if (!entry.workspaceCredits || credits.unlimited) return true;
  return Number(credits.balance) !== 0;
}
