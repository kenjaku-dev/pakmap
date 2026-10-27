// v0.9 feature e2e — PKGBUILD review inside the detail pane (cache-first
// states, bounded fetch flow, fail-open on refresh failure) and the
// package-URL row actions. All against the scripted mock gateway; fixtures
// reuse the existing LEGS names so the search totals stay untouched.
import { expect, test, type Locator, type Page } from "@playwright/test";

const sectionOf = (page: Page) => page.getByRole("region", { name: "PKGBUILD review" });

async function openDetail(page: Page, rowText: RegExp): Promise<Locator> {
  await page.goto("/");
  await page.getByLabel("Search packages").fill("firefox");
  await page.getByLabel("Search packages").press("Enter");
  await expect(page.getByRole("status").first()).toContainText(/Found/, { timeout: 3_000 });
  await page.getByRole("option").filter({ hasText: rowText }).first().click();
  const pane = page.getByRole("complementary", { name: "Package detail" });
  await expect(pane).toBeVisible();
  return pane;
}

test("PKGBUILD review: cached clone renders content, diff and actions", async ({ page }) => {
  await openDetail(page, /Mozilla Firefox web browser/);
  // AUR leg → firefox-bin (installed, cached clone with local diff)
  await page.getByRole("option").filter({ hasText: "firefox-bin" }).first().click();
  const section = sectionOf(page);
  await expect(section).toBeVisible();
  await expect(section.getByText("pkgname=firefox-bin").first()).toBeVisible();
  await expect(section.getByRole("button", { name: "Open on AUR" })).toBeVisible();
  await expect(section.getByRole("button", { name: "Copy PKGBUILD" })).toBeVisible();
  await section.getByText("Changes since last install").click();
  await expect(section).toContainText("+ pkgver=113.0");
});

test("PKGBUILD review: uncached package offers an explicit fetch, then renders fresh", async ({
  page,
}) => {
  await openDetail(page, /orphaned-thing/);
  const section = sectionOf(page);
  await expect(section).toBeVisible();
  await expect(section.getByText(/no cached clone for this package/)).toBeVisible();
  await section.getByRole("button", { name: "Fetch from AUR" }).click();
  await expect(section.getByText("pkgname=orphaned-thing")).toBeVisible();
  await expect(section.getByText("fresh from AUR")).toBeVisible();
});

test("package-URL row: open + copy actions for repository homepages", async ({ page }) => {
  const pane = await openDetail(page, /Mozilla Firefox web browser/);
  await expect(
    pane.getByRole("button", { name: /Open homepage: https:\/\/example\.org/ }),
  ).toBeVisible();
  await expect(pane.getByRole("button", { name: /Copy homepage URL/ })).toBeVisible();
  // validation lives in Rust (unit-tested); the mock opener is a no-op, so
  // clicking here only proves the wire round-trip holds.
  await pane.getByRole("button", { name: /Open homepage: https:\/\/example\.org/ }).click();
  await expect(pane).toBeVisible();
});

// B6 (v0.9.1): the stats row states how old the AUR count may be. The mock
// fixture checks 2 minutes ago (→ "just now"); the fixture has no per-call
// scenario hooks, so the fmt label is asserted loosely and the error-surfacing
// (aur_err banner) stays covered by the Rust tests.
test("AUR-checked chip shows a relative age in the stats row", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "Updates" }).click();
  const chips = page.getByTestId("stats-chips");
  await expect(chips).toBeVisible();
  await expect(chips).toContainText("AUR checked");
  await expect(chips).toContainText(/just now|\d+ ?(s|m|h|d) ?ago/);
});
