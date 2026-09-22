import { formatUsd } from "$lib/usageFormat";
import { isUnpriced, type ModelTotals } from "$lib/usageMerge";

/**
 * A model's cost, or "unpriced" when the rate table has no price for it yet: its tokens are
 * counted, its cost is not, and $0.00 would read as free.
 */
export function ModelCost({ row, className }: { row: Pick<ModelTotals, "costUsd" | "records" | "unpricedRecords">; className: string }) {
  if (isUnpriced(row)) {
    return (
      <span className={`${className} text-[var(--mute)]`} title="No public price for this model yet. Its tokens are counted; its cost is not.">
        unpriced
      </span>
    );
  }
  return <span className={className}>{formatUsd(row.costUsd)}</span>;
}
