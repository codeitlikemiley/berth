import { expect, test } from "@playwright/test";

import { lease, mount } from "./harness";

/**
 * Two separate defects lived here. Occupancy rendered as $0.000804 -- exact and
 * unreadable -- and the summary said "Income (forfeited -> $0)" unconditionally,
 * claiming forfeiture over income that was live and non-zero.
 */
test.describe("occupancy money", () => {
  test("the rate leads, the exact accrued figure follows", async ({ page }) => {
    await mount(page);
    await page.goto("/");
    // 0.00134 gas/s x $0.01 x 3600 = $0.048/hr, and x 60s minimum = $0.000804.
    await expect(page.getByText("$0.048/hr").first()).toBeVisible();
    await expect(page.getByText("$0.000804 accrued").first()).toBeVisible();
    await expect(page.getByText("quoted, not charged").first()).toBeVisible();
  });

  test("no forfeited lease means no talk of forfeiture", async ({ page }) => {
    await mount(page, { leases: [lease()] });
    await page.goto("/");
    const summary = page.getByText(/^Income/).first();
    await expect(summary).toBeVisible();
    await expect(page.getByText("forced disconnects earn $0")).toHaveCount(0);
  });

  test("a forfeited lease earns nothing, and the summary says why", async ({ page }) => {
    await mount(page, {
      leases: [
        lease({
          lease_id: "l_forf",
          live: false,
          status: "stopped",
          forfeited: true,
          end_reason: "forced",
          billable_seconds: 60,
        }),
      ],
    });
    await page.goto("/");
    await expect(page.getByText("forfeited").first()).toBeVisible();
    await expect(page.getByText("No income — forced disconnect.")).toBeVisible();
    // Occupancy is still recorded; only the income is zero.
    await expect(page.getByText("$0 accrued").first()).toBeVisible();
    await expect(page.getByText("forced disconnects earn $0")).toBeVisible();
  });
});
