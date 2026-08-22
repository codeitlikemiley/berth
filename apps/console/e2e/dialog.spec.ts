import { expect, test } from "@playwright/test";

import { expectDialogOpen, lease, mount } from "./harness";

/**
 * Force disconnect used to call window.confirm(). That blocks the event loop,
 * ignores the theme, and freezes any automation driving the page -- verifying
 * it by hand meant overriding window.confirm first. These assertions are the
 * reasons a real dialog was worth building.
 */
test.describe("confirm dialog", () => {
  test("destructive actions ask first, and cancelling does nothing", async ({ page }) => {
    const backend = await mount(page);
    await page.goto("/");

    await page.getByRole("button", { name: "Force disconnect" }).first().click();
    const dialog = await expectDialogOpen(page, "Force disconnect");
    // The copy is a product promise: occupancy is recorded, income is zero,
    // nothing is charged.
    await expect(dialog).toContainText("Host income for this lease is $0");
    await expect(dialog).toContainText("Nothing is charged");

    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    expect(backend.calls.some((c) => c.includes("/force"))).toBe(false);
    await expect(page.getByText("live").first()).toBeVisible();
  });

  test("Escape and the backdrop both cancel", async ({ page }) => {
    const backend = await mount(page);
    await page.goto("/");

    await page.getByRole("button", { name: "Force disconnect" }).first().click();
    await expectDialogOpen(page, "Force disconnect");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);

    await page.getByRole("button", { name: "Force disconnect" }).first().click();
    await expectDialogOpen(page, "Force disconnect");
    // Top-left is backdrop; the panel is centred.
    await page.mouse.click(8, 8);
    await expect(page.getByRole("dialog")).toHaveCount(0);

    expect(backend.calls.some((c) => c.includes("/force"))).toBe(false);
  });

  test("focus enters the dialog, is trapped, and returns to the trigger", async ({ page }) => {
    await mount(page);
    await page.goto("/");

    const trigger = page.getByRole("button", { name: "Force disconnect" }).first();
    await trigger.click();
    const dialog = await expectDialogOpen(page, "Force disconnect");

    // Cancel is the safe default for a destructive action.
    await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();

    // Tab must cycle inside rather than escaping to the page behind.
    await page.keyboard.press("Tab");
    await expect(dialog.getByRole("button", { name: "Force disconnect" })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();
  });

  test("the page behind cannot scroll while it is open", async ({ page }) => {
    await mount(page);
    await page.goto("/");
    await expect(page.locator("body")).not.toHaveCSS("overflow", "hidden");
    await page.getByRole("button", { name: "Force disconnect" }).first().click();
    await expectDialogOpen(page, "Force disconnect");
    await expect(page.locator("body")).toHaveCSS("overflow", "hidden");
    await page.keyboard.press("Escape");
    await expect(page.locator("body")).not.toHaveCSS("overflow", "hidden");
  });

  test("confirming forfeits the lease", async ({ page }) => {
    const backend = await mount(page);
    await page.goto("/");

    await page.getByRole("button", { name: "Force disconnect" }).first().click();
    const dialog = await expectDialogOpen(page, "Force disconnect");
    await dialog.getByRole("button", { name: "Force disconnect" }).click();

    await expect(page.getByText("forfeited")).toBeVisible();
    await expect(page.getByText("No income — forced disconnect.")).toBeVisible();
    expect(backend.calls.some((c) => c === "POST /v1/leases/l_e2e_1/force")).toBe(true);
  });

  test("revoke on doctor asks too, in its own words", async ({ page }) => {
    await mount(page, { leases: [lease({ live: false, status: "stopped" })] });
    await page.goto("/doctor");
    await page.getByRole("button", { name: "Revoke other clients" }).click();
    const dialog = await expectDialogOpen(page, "Revoke other clients?");
    await expect(dialog).toContainText("CLI and other browsers must pair again");
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
  });
});
