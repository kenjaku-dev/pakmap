// AppImage tab e2e — the scripted mock gateway acts as the backend, so no
// AppImage is executed and no real download happens. Covers the full UI flow
// plus one axe scan of the new tab.
import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

async function gotoAppImages(page: import("@playwright/test").Page): Promise<void> {
  await page.goto("/");
  await page.getByRole("tab", { name: "AppImages" }).click();
  await expect(page.getByRole("tabpanel", { name: "AppImages" })).toBeVisible();
}

test("AppImages tab lists the integrated fixture and passes axe", async ({ page }) => {
  await gotoAppImages(page);
  await expect(page.getByRole("list", { name: "Integrated AppImages" })).toContainText(
    "Firefox Browser",
  );
  const { violations } = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
    .analyze();
  expect(violations.map((v) => v.id)).toEqual([]);
});

test("integrate adds a row through the IPC facade", async ({ page }) => {
  await gotoAppImages(page);
  await page.getByLabel("AppImage path").fill("/home/me/Downloads/App.AppImage");
  await page.getByRole("button", { name: "Integrate" }).click();
  await expect(page.getByRole("tabpanel", { name: "AppImages" }).getByRole("status")).toContainText("Integrated App");
  await expect(page.getByRole("list", { name: "Integrated AppImages" })).toContainText("App");
});

test("URL install requires a checksum and then adds a row", async ({ page }) => {
  await gotoAppImages(page);
  await page.getByLabel("HTTPS URL").fill("https://example.org/App.AppImage");
  const install = page.getByRole("button", { name: "Install" });
  await expect(install).toBeDisabled();
  await page.getByLabel("SHA-256 (64 hex)").fill("ab".repeat(32));
  await expect(install).toBeEnabled();
  await install.click();
  await expect(page.getByRole("tabpanel", { name: "AppImages" }).getByRole("status")).toContainText("Installed App");
});

test("removing a row asks first, then calls the backend and clears it", async ({ page }) => {
  await gotoAppImages(page);
  await page.getByRole("button", { name: "Remove Firefox Browser" }).click();
  const dialog = page.getByRole("dialog", { name: "Remove Firefox Browser" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Remove", exact: true }).click();
  await expect(page.getByRole("tabpanel", { name: "AppImages" }).getByRole("status")).toContainText("Removed Firefox Browser");
  await expect(page.getByText("None yet.")).toBeVisible();
});

test("hotkey 4 opens the AppImages tab", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "News" }).click();
  await page.keyboard.press("4");
  await expect(page.getByRole("tabpanel", { name: "AppImages" })).toBeVisible();
});
