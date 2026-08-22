import { expect, test } from "@playwright/test";

import { mount, node } from "./harness";

/**
 * The wizard used to accept 9999 vCPU, walk you to Where, price it, and only
 * fail at Confirm in Docker's words. The node knows its capacity from the
 * start.
 */
test.describe("lease wizard capacity gate", () => {
  const step = (p: import("@playwright/test").Page) => p.getByText(/[0-9] \/ 3 ·/);

  test("shows the bound before anyone hits it", async ({ page }) => {
    await mount(page);
    await page.goto("/leases/new");
    await expect(page.getByText("this node has 8 vCPU · 7 GiB")).toBeVisible();
  });

  test("refuses a draft larger than the node, at step one", async ({ page }) => {
    await mount(page);
    await page.goto("/leases/new");

    // The refusal is immediate: the message appears as you type and Next is
     // disabled, so there is no way to carry an impossible draft forward.
    await page.getByLabel("vCPU").fill("9999");
    await expect(
      page.getByText("vcpu 9999 exceeds this node's 8 available CPUs"),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Next" })).toBeDisabled();
    await expect(step(page)).toContainText("1 / 3");

    await page.getByLabel("vCPU").fill("2");
    await page.getByLabel("mem GiB").fill("9999");
    await expect(
      page.getByText("mem_gib 9999 exceeds this node's 7 GiB of memory"),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Next" })).toBeDisabled();
    await expect(step(page)).toContainText("1 / 3");
  });

  test("zero is still rejected, and is not read as unlimited", async ({ page }) => {
    await mount(page);
    await page.goto("/leases/new");
    await page.getByLabel("vCPU").fill("0");
    await expect(
      page.getByText("vcpu and mem_gib must be greater than zero (0 is not unlimited)"),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Next" })).toBeDisabled();
    await expect(step(page)).toContainText("1 / 3");
  });

  test("exactly at capacity is allowed — it is a limit, not a forbidden value", async ({ page }) => {
    await mount(page);
    await page.goto("/leases/new");
    await page.getByLabel("vCPU").fill("8");
    await page.getByLabel("mem GiB").fill("7");
    await page.getByRole("button", { name: "Next" }).click();
    await expect(step(page)).toContainText("2 / 3");
  });

  test("unknown capacity does not block — a failed probe is not a zero", async ({ page }) => {
    await mount(page, { node: node({ capacity: { vcpu: null, mem_gib: null } }) });
    await page.goto("/leases/new");
    await expect(page.getByText(/this node has/)).toHaveCount(0);
    await page.getByLabel("vCPU").fill("9999");
    await page.getByRole("button", { name: "Next" }).click();
    await expect(step(page)).toContainText("2 / 3");
  });
});
