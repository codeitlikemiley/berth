import { describe, expect, it } from "vitest";

import type { LeaseView, Quote } from "@/api/types";
import {
  FORCE_BADGE_COPY,
  incomeUsd,
  occupancyBadge,
  occupancyBadgeCopy,
  quotedUsd,
  usdPerSecond,
} from "./ledger";

/** MATH.md 2/4/40 isolated Linux seed as the node serializes Quote strings. */
const isolated240: Quote = {
  vcpu: 2,
  mem_gib: 4,
  disk_gib: 40,
  os: "linux",
  os_mult: 1,
  density: "isolated",
  density_mult: 1,
  term: "on_demand",
  min_seconds: 60,
  pooled: false,
  gas_per_second: "0.00134",
  currency: "gas",
  usd_per_gas: "0.01",
};

function row(partial: Partial<LeaseView> = {}): LeaseView {
  return {
    lease_id: "l_test",
    session_id: "s_test",
    ws_url: "ws://127.0.0.1:7432/v1/sessions/s_test",
    quote: isolated240,
    status: "stopped",
    billable_seconds: 60,
    elapsed_seconds: 60,
    started_at: 1_700_000_000,
    workspace_id: "ws_test",
    live: false,
    forfeited: false,
    end_reason: "graceful",
    ...partial,
  };
}

describe("ledger 2/4/40 isolated", () => {
  it("usd_per_second is 0.0000134", () => {
    expect(usdPerSecond(isolated240)).toBeCloseTo(0.0000134, 12);
  });

  it("stopped graceful billable_seconds=60 → occupancy $0.000804 and income $0.000804", () => {
    const lease = row({
      status: "stopped",
      live: false,
      billable_seconds: 60,
      forfeited: false,
      end_reason: "graceful",
    });
    expect(quotedUsd(lease)).toBe(0.000804);
    expect(incomeUsd(lease)).toBe(0.000804);
    expect(occupancyBadge(lease)).toBe("stopped");
  });

  it("same row forfeited: occupancy still $0.000804, income $0", () => {
    const lease = row({
      status: "stopped",
      live: false,
      billable_seconds: 60,
      forfeited: true,
      end_reason: "forced",
    });
    expect(quotedUsd(lease)).toBe(0.000804);
    expect(incomeUsd(lease)).toBe(0);
    expect(occupancyBadge(lease)).toBe("forfeited");
    expect(occupancyBadgeCopy(lease)).toBe("No income — forced disconnect.");
    expect(FORCE_BADGE_COPY).toBe("No income — forced disconnect.");
  });

  it("live 3600s → occupancy $0.04824", () => {
    const nowUnix = 1_700_003_600;
    const lease = row({
      status: "active",
      live: true,
      started_at: nowUnix - 3600,
      billable_seconds: null,
      elapsed_seconds: null,
      forfeited: false,
      end_reason: null,
    });
    expect(quotedUsd(lease, nowUnix)).toBeCloseTo(0.04824, 10);
    expect(occupancyBadge(lease)).toBe("live");
  });

  it("active && live === false does not increase when nowUnix advances", () => {
    const lease = row({
      status: "active",
      live: false,
      elapsed_seconds: 90,
      billable_seconds: null,
      forfeited: false,
      end_reason: null,
      started_at: 0,
    });
    const atT = quotedUsd(lease, 1_000);
    const later = quotedUsd(lease, 10_000);
    expect(later).toBe(atT);
    expect(atT).toBeCloseTo(usdPerSecond(isolated240) * 90, 12);
  });

  it("stale is not treated as forfeited", () => {
    const lease = row({
      status: "active",
      live: false,
      elapsed_seconds: 60,
      forfeited: false,
      end_reason: null,
    });
    expect(occupancyBadge(lease)).toBe("stale");
    expect(occupancyBadgeCopy(lease)).toBe("stale");
    expect(incomeUsd(lease)).toBe(quotedUsd(lease));
    expect(incomeUsd(lease)).not.toBe(0);
  });
});
