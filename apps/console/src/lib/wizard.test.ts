import { describe, expect, it } from "vitest";

import {
  UNPARKED_LEASE_COPY,
  quoteMatchesDraft,
  wizardMayPost,
  wizardWhatReady,
  wizardWhereNextEnabled,
} from "./wizard";

const draft240 = {
  vcpu: 2,
  mem_gib: 4,
  disk_gib: 40,
  density: "isolated",
};

describe("lease wizard gates", () => {
  it("rejects vcpu or mem_gib of 0", () => {
    expect(wizardWhatReady(0, 4)).toBe(false);
    expect(wizardWhatReady(2, 0)).toBe(false);
    expect(wizardWhatReady(2, 4)).toBe(true);
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
