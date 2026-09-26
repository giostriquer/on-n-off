import type { LimitsCreditsSpent, LimitsWorkspaceCredits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import type { CardFigures } from "./limitCards";
import { presentCreditsSpent, presentWorkspaceShare } from "./limitPresentation";
import { MeterRow } from "./Meter";
import { SummaryRow } from "./SummaryRow";

/**
 * The account's credits as rows under the windows, so they never crowd the header's identity: the
 * account's own balance, the member's share of a business workspace's pooled credits (Codex's spend
 * control) and what the member spent lately. Which of them a card shows is the card model's call
 * (`CardFigures`).
 */
export function CreditsRows({ figures, provider, now }: {
  figures: Pick<CardFigures, "ownBalance" | "workspaceShare" | "creditsSpent">;
  provider: AgentId;
  now: number;
}) {
  const { ownBalance, workspaceShare: share, creditsSpent: spent } = figures;
  return (
    <>
      {ownBalance ? <SummaryRow label="Credits" value={ownBalance.unlimited ? "Unlimited" : ownBalance.balance} /> : null}
      {share ? <WorkspaceShareRow share={share} provider={provider} now={now} /> : null}
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
