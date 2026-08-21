import { describe, expect, it } from "vitest";

import { leaseRequestBody } from "./node";

describe("leaseRequestBody", () => {
  it("matches mvp_lease_request fields and omits network", () => {
    const body = leaseRequestBody({
      os: "linux",
      density: "isolated",
      resources: { vcpu: 2, mem_gib: 4, disk_gib: 40 },
    });
    expect(body).toEqual({
      os: "linux",
      class: "private",
      license: "linux",
      density: "isolated",
      term: "on_demand",
      resources: { vcpu: 2, mem_gib: 4, disk_gib: 40 },
    });
    expect(body).not.toHaveProperty("network");
    expect(JSON.stringify(body)).not.toMatch(/windows/i);
  });

  it("forwards shared density from the wizard", () => {
    const body = leaseRequestBody({
      os: "linux",
      density: "shared",
      resources: { vcpu: 2, mem_gib: 4, disk_gib: 40 },
    });
    expect(body.density).toBe("shared");
    expect(body.os).toBe("linux");
  });
});
