import { formatShortDate, formatUsedPercent, hasElapsed, usageTextColor } from "$lib/limitsFormat";
import type { LimitsWorkspaceCredits, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { formatAgo } from "$lib/timeFormat";
import { workspaceSharePercent } from "./limitPresentation";
import { MeterRow } from "./Meter";
import { SummaryRow } from "./SummaryRow";

const AMOUNT = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

/**
 * The account's credits as rows under the windows, so they never crowd the header's identity: the
 * member's share of a business workspace's pooled credits (Codex's spend control), and the
 * account's own balance. In a workspace the credits are the workspace's, so the own balance reads 0
 * beside a share that says what the member can use; it is left out then.
 */
export function CreditsRows({ entry, now }: { entry: Pick<ProviderLimits, "provider" | "credits" | "workspaceCredits">; now: number }) {
  const share = entry.workspaceCredits;
  const credits = entry.credits;
  const ownBalance = credits && (!share || credits.unlimited || Number(credits.balance) !== 0) ? credits : null;
  return (
    <>
      {ownBalance ? <SummaryRow label="Credits" value={ownBalance.unlimited ? "Unlimited" : ownBalance.balance} /> : null}
      {share ? <WorkspaceShareRow share={share} provider={entry.provider} now={now} /> : null}
    </>
  );
}

/**
 * The share as a meter row, like a window's: the bar and figure say how much of it is used, the note
 * what is left and when it resets. A share whose reset has passed has renewed, and reads as a window
 * whose reset has passed does: nothing used, and when it reset.
 */
function WorkspaceShareRow({ share, provider, now }: { share: LimitsWorkspaceCredits; provider: AgentId; now: number }) {
  const renewed = hasElapsed(share.resetsAt, now);
  const percent = workspaceSharePercent(share, now);
  const limit = Number(share.limit);
  const left = renewed ? limit : Math.max(limit - Number(share.used), 0);
  const amounts = !renewed && share.reached
    ? `all ${AMOUNT.format(limit)} used`
    : `${AMOUNT.format(left)} of ${AMOUNT.format(limit)} left`;
  const resetDate = formatShortDate(share.resetsAt);
  const reset = renewed ? `reset ${formatAgo(share.resetsAt, now)} · ${resetDate}` : resetDate ? `resets ${resetDate}` : undefined;
  return (
    <MeterRow
      label="Workspace credits"
      note={[amounts, reset].filter(Boolean).join(" · ")}
      percent={percent}
      text={formatUsedPercent(percent)}
      color={usageTextColor(percent)}
      provider={provider}
    />
  );
}
