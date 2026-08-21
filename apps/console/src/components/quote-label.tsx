import { formatUsd, formatUsdRate } from "@/lib/ledger";

export function QuoteLabel({
  usd,
  ratePerHour,
}: {
  usd: number;
  ratePerHour: number;
}) {
  return (
    <span className="inline-flex flex-col gap-0.5">
      <span className="font-medium tabular-nums">{formatUsdRate(ratePerHour)}</span>
      <span className="text-xs text-muted-foreground">
        {formatUsd(usd)} accrued · quoted, not charged
      </span>
    </span>
  );
}
