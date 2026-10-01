import type { AgentId, InstallItemsResult, ItemOutcome } from "./types";

export type OutcomeSummary = {
  installed: number;
  skipped: number;
  conflicts: ItemOutcome[];
  failed: ItemOutcome[];
  touchedProviders: AgentId[];
};

export function summarizeOutcomes(result: InstallItemsResult): OutcomeSummary {
  const touched = new Set<AgentId>();
  let installed = 0;
  let skipped = 0;
  const conflicts: ItemOutcome[] = [];
  const failed: ItemOutcome[] = [];
  for (const outcome of result.outcomes) {
    switch (outcome.status) {
      case "installed":
      case "replaced":
        installed += 1;
        touched.add(outcome.provider);
        break;
      case "skipped":
        skipped += 1;
        break;
      case "conflict":
        conflicts.push(outcome);
        break;
      case "failed":
        failed.push(outcome);
        break;
    }
  }
  return { installed, skipped, conflicts, failed, touchedProviders: [...touched] };
}

export function installOutcomeClean(result: InstallItemsResult): boolean {
  const summary = summarizeOutcomes(result);
  return summary.conflicts.length === 0 && summary.failed.length === 0;
}
