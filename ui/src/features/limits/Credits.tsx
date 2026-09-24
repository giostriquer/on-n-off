import { formatShortDate, hasElapsed } from "$lib/limitsFormat";
import type { ProviderLimits } from "$lib/limitsTypes";
import { formatAgo } from "$lib/timeFormat";
import { SummaryRow } from "./SummaryRow";

const AMOUNT = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

/**
 * The account's credits as rows under the windows, so they never crowd the header's identity: the
 * member's share of a business workspace's pooled credits (Codex's spend control), and the
 * account's own balance. In a workspace the credits are the workspace's, so the own balance reads 0
 * beside a share that says what the member can use; it is left out then.
 */
export function CreditsRows({ entry, now }: { entry: Pick<ProviderLimits, "credits" | "workspaceCredits">; now: number }) {
  const share = entry.workspaceCredits;
  const credits = entry.credits;
  const ownBalance = credits && (!share || credits.unlimited || Number(credits.balance) !== 0) ? credits : null;
  return (
    <>
      {ownBalance ? <SummaryRow label="Credits" value={ownBalance.unlimited ? "Unlimited" : ownBalance.balance} /> : null}
      {share ? <WorkspaceShareRow share={share} now={now} /> : null}
    </>
  );
}

/**
 * What is left of the share and when it resets. A share whose reset has passed has renewed, and reads
 * as a window whose reset has passed does: nothing used, and when it reset.
 */
function WorkspaceShareRow({ share, now }: { share: NonNullable<ProviderLimits["workspaceCredits"]>; now: number }) {
  const renewed = hasElapsed(share.resetsAt, now);
  const limit = Number(share.limit);
  const used = renewed ? 0 : Number(share.used);
  const reached = !renewed && share.reached;
  const value = reached ? "Used up" : `${AMOUNT.format(Math.max(limit - used, 0))} of ${AMOUNT.format(limit)} left`;
  const resetDate = formatShortDate(share.resetsAt);
  const note = renewed
    ? `reset ${formatAgo(share.resetsAt, now)} · ${resetDate}`
    : [reached ? `all ${AMOUNT.format(limit)} used` : undefined, resetDate ? `resets ${resetDate}` : undefined].filter(Boolean).join(" · ");
  return <SummaryRow label="Workspace credits" value={value} note={note || undefined} />;
}
