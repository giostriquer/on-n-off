import { formatUsd } from "$lib/usageFormat";
import { isUnpriced, type CostTally } from "$lib/usageMerge";

export function UsageCost({ row, className }: { row: Pick<CostTally, "costUsd" | "records" | "unpricedRecords">; className: string }) {
  if (isUnpriced(row)) {
    return (
      <span className={`${className} text-[var(--mute)]`} title="No public price for these tokens yet. They are counted; their cost is not.">
        unpriced
      </span>
    );
  }
  return <span className={className}>{formatUsd(row.costUsd)}</span>;
}
