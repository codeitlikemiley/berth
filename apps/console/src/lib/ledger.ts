import type { LeaseView, Quote } from "@/api/types";

/** Matches Quote::usd_per_second in crates/berth-protocol/src/quote.rs */
export function usdPerSecond(q: Quote): number {
  return Number(q.gas_per_second) * Number(q.usd_per_gas);
}

/** Quote has no elapsed clock yet; review uses rate × min_seconds. */
export function occupancyUsd(quote: Quote): number {
  return usdPerSecond(quote) * quote.min_seconds;
}

export function quotedUsd(lease: LeaseView, nowUnix = Date.now() / 1000): number {
  const min = lease.quote.min_seconds;
  const rate = usdPerSecond(lease.quote);
  if (lease.status === "stopped") {
    return rate * (lease.billable_seconds ?? min);
  }
  if (!lease.live) {
    return rate * (lease.elapsed_seconds ?? min);
  }
  const elapsed = Math.max(0, nowUnix - lease.started_at);
  return rate * Math.max(min, elapsed);
}

export function occupancyBadge(lease: LeaseView): "live" | "stopped" | "stale" | "forfeited" {
  if (lease.forfeited || lease.end_reason === "forced") return "forfeited";
  if (lease.status === "stopped") return "stopped";
  if (!lease.live) return "stale";
  return "live";
}

export function incomeUsd(lease: LeaseView, nowUnix = Date.now() / 1000): number {
  if (lease.forfeited || lease.end_reason === "forced") return 0;
  return quotedUsd(lease, nowUnix);
}

export const FORCE_BADGE_COPY = "No income — forced disconnect.";

export const FORCE_CONFIRM_COPY =
  "This stops the guest. Occupancy time is recorded. Host income for this lease is $0. Nothing is charged.";

export function occupancyBadgeCopy(lease: LeaseView): string {
  if (occupancyBadge(lease) === "forfeited") return FORCE_BADGE_COPY;
  return occupancyBadge(lease);
}

export function formatUsd(n: number): string {
  if (n === 0) return "$0";
  return `$${n.toFixed(6).replace(/0+$/, "").replace(/\.$/, "")}`;
}
