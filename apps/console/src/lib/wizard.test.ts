import { describe, expect, it } from "vitest";

import {
  UNPARKED_LEASE_COPY,
  wizardMayPost,
  wizardWhatReady,
  wizardWhereNextEnabled,
} from "./wizard";

describe("lease wizard gates", () => {
  it("rejects vcpu or mem_gib of 0", () => {
    expect(wizardWhatReady(0, 4)).toBe(false);
    expect(wizardWhatReady(2, 0)).toBe(false);
    expect(wizardWhatReady(2, 4)).toBe(true);
  });

  it("unparked node cannot leave Where", () => {
    expect(wizardWhereNextEnabled(false)).toBe(false);
    expect(wizardWhereNextEnabled(true)).toBe(true);
    expect(UNPARKED_LEASE_COPY).toBe(
      "Park this node before leasing (inventory is off).",
    );
  });

  it("Windows option does not POST", () => {
    expect(wizardMayPost("windows")).toBe(false);
    expect(wizardMayPost("macos")).toBe(false);
    expect(wizardMayPost("linux")).toBe(true);
  });
});
