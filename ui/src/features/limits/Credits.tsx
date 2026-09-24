import type { LimitsWorkspaceCredits, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { presentWorkspaceShare } from "./limitPresentation";
import { MeterRow } from "./Meter";
import { SummaryRow } from "./SummaryRow";

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

/** The share as a meter row, like a window's (`presentWorkspaceShare`). */
function WorkspaceShareRow({ share, provider, now }: { share: LimitsWorkspaceCredits; provider: AgentId; now: number }) {
  const { percent, text, color, note } = presentWorkspaceShare(share, now);
  return <MeterRow label="Workspace credits" note={note} percent={percent} text={text} color={color} provider={provider} />;
}
