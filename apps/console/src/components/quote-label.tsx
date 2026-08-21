import { formatUsd } from "@/lib/ledger";

export function QuoteLabel({ usd }: { usd: number }) {
  return (
    <span className="inline-flex flex-wrap items-baseline gap-x-1.5">
      <span>{formatUsd(usd)}</span>
      <span className="text-xs text-muted-foreground">quoted, not charged</span>
    </span>
  );
}
