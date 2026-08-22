import { describe, expect, it } from "vitest";

import {
  INVALID_RESOURCES_COPY,
  UNPARKED_LEASE_COPY,
  capacityHint,
  quoteMatchesDraft,
  wizardMayPost,
  wizardWhatError,
  wizardWhereNextEnabled,
} from "./wizard";

const CAP = { vcpu: 8, mem_gib: 7 };

const draft240 = {
  vcpu: 2,
  mem_gib: 4,
  disk_gib: 40,
  density: "isolated",
};

describe("lease wizard gates", () => {
  it("rejects vcpu or mem_gib of 0", () => {
    expect(wizardWhatError(0, 4, CAP)).toBe(INVALID_RESOURCES_COPY);
    expect(wizardWhatError(2, 0, CAP)).toBe(INVALID_RESOURCES_COPY);
    expect(wizardWhatError(2, 4, CAP)).toBeNull();
  });

  it("refuses a draft the node cannot host, at step one", () => {
    expect(wizardWhatError(9999, 4, CAP)).toBe(
      "vcpu 9999 exceeds this node's 8 available CPUs",
    );
    expect(wizardWhatError(2, 9999, CAP)).toBe(
      "mem_gib 9999 exceeds this node's 7 GiB of memory",
    );
    // Exactly at capacity is allowed; the node only refuses above it.
    expect(wizardWhatError(8, 7, CAP)).toBeNull();
  });

  it("unknown capacity does not block — a failed probe is not a zero", () => {
    expect(wizardWhatError(9999, 9999, null)).toBeNull();
    expect(wizardWhatError(9999, 4, { vcpu: null, mem_gib: 7 })).toBeNull();
    expect(wizardWhatError(2, 9999, { vcpu: 8, mem_gib: null })).toBeNull();
    // A zero draft is still rejected without any capacity information.
    expect(wizardWhatError(0, 4, null)).toBe(INVALID_RESOURCES_COPY);
  });

  it("shows the bound before anyone hits it", () => {
    expect(capacityHint(CAP)).toBe("this node has 8 vCPU · 7 GiB");
    expect(capacityHint({ vcpu: 8, mem_gib: null })).toBe("this node has 8 vCPU");
    expect(capacityHint({ vcpu: null, mem_gib: null })).toBeNull();
    expect(capacityHint(null)).toBeNull();
  });

  it("unparked node cannot leave Where; failed load can retry", () => {
    expect(wizardWhereNextEnabled({ parked: false }, false)).toBe(false);
    expect(wizardWhereNextEnabled({ parked: true }, false)).toBe(true);
    expect(wizardWhereNextEnabled({ parked: true }, true)).toBe(false);
    expect(wizardWhereNextEnabled(null, true)).toBe(false);
    expect(wizardWhereNextEnabled(null, false)).toBe(true);
    expect(UNPARKED_LEASE_COPY).toBe(
      "Park this node before leasing (inventory is off).",
    );
  });

  it("Windows option does not POST", () => {
    expect(wizardMayPost("windows")).toBe(false);
    expect(wizardMayPost("macos")).toBe(false);
    expect(wizardMayPost("linux")).toBe(true);
  });

  it("quote is usable only when it matches the draft", () => {
    expect(quoteMatchesDraft(draft240, draft240)).toBe(true);
    expect(
      quoteMatchesDraft({ ...draft240, vcpu: 4 }, draft240),
    ).toBe(false);
    expect(
      quoteMatchesDraft({ ...draft240, density: "shared" }, draft240),
    ).toBe(false);
  });
});
