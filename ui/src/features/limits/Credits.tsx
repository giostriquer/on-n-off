import type { LimitsCreditsSpent, LimitsWorkspaceCredits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import type { CardFigures } from "./limitCards";
import { presentCreditsSpent, presentWorkspaceShare } from "./limitPresentation";
import { MeterRow } from "./Meter";
import { SummaryRow } from "./SummaryRow";

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

function CreditsSpentRow({ spent }: { spent: LimitsCreditsSpent }) {
  const { value, note } = presentCreditsSpent(spent);
  return <SummaryRow label="Credits spent" value={value} note={note} />;
}

function WorkspaceShareRow({ share, provider, now }: { share: LimitsWorkspaceCredits; provider: AgentId; now: number }) {
  const { percent, text, color, note } = presentWorkspaceShare(share, now);
  return <MeterRow label="Workspace credits" note={note} percent={percent} text={text} color={color} provider={provider} />;
}
