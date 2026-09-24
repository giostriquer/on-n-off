import type { LimitsCreditsSpent, LimitsWorkspaceCredits, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { presentCreditsSpent, presentWorkspaceShare } from "./limitPresentation";
import { MeterRow } from "./Meter";
import { SummaryRow } from "./SummaryRow";

/**
 * The account's credits as rows under the windows, so they never crowd the header's identity: the
 * member's share of a business workspace's pooled credits (Codex's spend control), what the member
 * spent lately, and the account's own balance. In a workspace the credits are the workspace's, so
 * the own balance reads 0 beside a share or spending that says what the member actually has; it is
 * left out then.
 */
export function CreditsRows({ entry, now }: { entry: Pick<ProviderLimits, "provider" | "credits" | "workspaceCredits" | "creditsSpent">; now: number }) {
  const share = entry.workspaceCredits;
  const spent = entry.creditsSpent;
  const credits = entry.credits;
  const ownBalance = credits && (!(share || spent) || credits.unlimited || Number(credits.balance) !== 0) ? credits : null;
  return (
    <>
      {ownBalance ? <SummaryRow label="Credits" value={ownBalance.unlimited ? "Unlimited" : ownBalance.balance} /> : null}
      {share ? <WorkspaceShareRow share={share} provider={entry.provider} now={now} /> : null}
      {spent ? <CreditsSpentRow spent={spent} /> : null}
    </>
  );
}

/** What the member spent lately (`presentCreditsSpent`); spending has no limit to meter against. */
function CreditsSpentRow({ spent }: { spent: LimitsCreditsSpent }) {
  const { value, note } = presentCreditsSpent(spent);
  return <SummaryRow label="Credits spent" value={value} note={note} />;
}

/** The share as a meter row, like a window's (`presentWorkspaceShare`). */
function WorkspaceShareRow({ share, provider, now }: { share: LimitsWorkspaceCredits; provider: AgentId; now: number }) {
  const { percent, text, color, note } = presentWorkspaceShare(share, now);
  return <MeterRow label="Workspace credits" note={note} percent={percent} text={text} color={color} provider={provider} />;
}
