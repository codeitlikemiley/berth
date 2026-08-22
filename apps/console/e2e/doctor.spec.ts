import { expect, test } from "@playwright/test";

import { lease, mount } from "./harness";

/**
 * Doctor fetched once on mount and never again, so a node that died after load
 * kept reporting "docker ok" forever. It was also unreachable by clicking:
 * the route existed with no link to it anywhere in the app.
 */
test.describe("doctor", () => {
  test("is reachable by clicking, not only by typing the URL", async ({ page }) => {
    await mount(page);
    await page.goto("/");
    await page.getByRole("link", { name: "Doctor" }).click();
    await expect(page).toHaveURL(/\/doctor$/);
    // Asserting on the page's own copy rather than a heading role: CardTitle
     // renders a div, so the console has no headings outside dialogs.
    await expect(page.getByText("Node health from this process.")).toBeVisible();
  });

  test("re-reads health rather than remembering it", async ({ page }) => {
    await mount(page, { leases: [lease({ live: false, status: "stopped" })] });
    await page.goto("/doctor");

    const stamp = page.getByText(/^checked \d\d:\d\d:\d\d$/);
    await expect(stamp).toBeVisible();
    const first = await stamp.textContent();
    // The stamp must move on its own; a snapshot presented as live status is
    // the bug this replaced.
    await expect(async () => {
      expect(await stamp.textContent()).not.toBe(first);
    }).toPass({ timeout: 15_000 });
  });

  test("a node that goes away stops reading green", async ({ page }) => {
    const backend = await mount(page, { leases: [lease({ live: false, status: "stopped" })] });
    await page.goto("/doctor");
    await expect(page.getByText("ok bollard ping ok")).toBeVisible();

    backend.down = true;
    await expect(page.getByText(/node unreachable; readings above are from/)).toBeVisible({
      timeout: 15_000,
    });

    // It still shows the last known reading, but says so rather than implying
    // it just checked.
    backend.down = false;
    await expect(page.getByText(/node unreachable/)).toHaveCount(0, { timeout: 15_000 });
  });

  test("says why an image is usable, not merely that it exists", async ({ page }) => {
    await mount(page, { leases: [lease({ live: false, status: "stopped" })] });
    await page.goto("/doctor");
    await expect(page.getByText("egress filter v2")).toBeVisible();
  });
});
