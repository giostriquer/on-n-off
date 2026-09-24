import { formatShortDate } from "$lib/limitsFormat";
import type { ProviderLimits } from "$lib/limitsTypes";
import { currentWorkspaceShare } from "./limitPresentation";
import { SummaryRow } from "./SummaryRow";

const AMOUNT = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

/**
 * The account's credits as rows under the windows, so they never crowd the header's identity: the
 * member's share of a business workspace's pooled credits (Codex's spend control), and the
 * account's own balance. In a workspace the credits are the workspace's, so the own balance reads 0
 * beside a share that says what the member can use; it is left out then.
 */
export function CreditsRows({ entry, now }: { entry: Pick<ProviderLimits, "credits" | "workspaceCredits">; now: number }) {
  const share = currentWorkspaceShare(entry.workspaceCredits, now);
  const credits = entry.credits;
  const ownBalance = credits && (!share || credits.unlimited || Number(credits.balance) !== 0) ? credits : null;
  return (
    <>
      {ownBalance ? <SummaryRow label="Credits" value={ownBalance.unlimited ? "Unlimited" : ownBalance.balance} /> : null}
      {share ? <WorkspaceShareRow share={share} /> : null}
    </>
  );
}

function WorkspaceShareRow({ share }: { share: NonNullable<ProviderLimits["workspaceCredits"]> }) {
  const limit = Number(share.limit);
  const used = Number(share.used);
  const value = share.reached ? "Used up" : `${AMOUNT.format(Math.max(limit - used, 0))} of ${AMOUNT.format(limit)} left`;
  const note = [share.reached ? `all ${AMOUNT.format(limit)} used` : undefined, share.resetsAt ? `resets ${formatShortDate(share.resetsAt)}` : undefined]
    .filter(Boolean)
    .join(" · ");
  return <SummaryRow label="Workspace credits" value={value} note={note || undefined} />;
}
