import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  XAxis,
  YAxis,
} from "recharts";

import type { LeaseView } from "@/api/types";
import { QuoteLabel } from "@/components/quote-label";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { incomeUsd, quotedUsd } from "@/lib/ledger";

export function MeterChart({
  leases,
  nowUnix,
  truncated,
}: {
  leases: LeaseView[];
  nowUnix: number;
  truncated: boolean;
}) {
  const data = leases.map((lease) => ({
    lease_id: lease.lease_id,
    quotedUsd: quotedUsd(lease, nowUnix),
  }));
  const quotedSum = data.reduce((sum, row) => sum + row.quotedUsd, 0);
  const incomeSum = leases.reduce(
    (sum, lease) => sum + incomeUsd(lease, nowUnix),
    0,
  );

  return (
    <Card>
      <CardHeader>
        <CardTitle>Quoted occupancy</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="h-64 w-full">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={data}>
              <CartesianGrid stroke="var(--border)" />
              <XAxis
                dataKey="lease_id"
                tick={{ fill: "var(--text-muted)" }}
                stroke="var(--text-muted)"
              />
              <YAxis
                tick={{ fill: "var(--text-muted)" }}
                stroke="var(--text-muted)"
              />
              <Bar
                dataKey="quotedUsd"
                fill="var(--chart-1)"
                isAnimationActive={false}
              />
            </BarChart>
          </ResponsiveContainer>
        </div>
        <div className="flex flex-wrap gap-x-8 gap-y-2 text-sm">
          <p>
            Quoted occupancy (not charged). <QuoteLabel usd={quotedSum} />
          </p>
          <p>
            Income (forfeited → $0) <QuoteLabel usd={incomeSum} />
          </p>
        </div>
        {truncated ? (
          <p className="text-sm text-muted-foreground">latest 500 leases.</p>
        ) : null}
      </CardContent>
    </Card>
  );
}
