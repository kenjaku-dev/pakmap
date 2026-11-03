// v0.10.0 §3 settings e2e — the GUI's second door into the same engine:
// open via gear + hotkey, filter, origin flip on write, danger copy, conflict
// banner, the readonly → flags-refetch → actions-disable seam, and the D1
// theme lock row. Runs against the scripted mock gateway (tree-shaken path).
import { expect, test, type Page } from "@playwright/test";

const rowOf = (page: Page, key: string) => page.getByTestId(`setting-row-${key}`);

async function openSettingsViaGear(page: Page): Promise<void> {
  await page.goto("/");
  await page.getByTestId("open-settings").click();
  await expect(page.getByTestId("settings-screen")).toBeVisible();
}

test("opens via the header gear, closes with Escape", async ({ page }) => {
  await openSettingsViaGear(page);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("settings-screen")).toHaveCount(0);
});

// the search field auto-focuses on boot — the hotkey fires when no input owns
// focus (click empty header space first); typing `s` must never trigger it.
test("opens via the `s` hotkey when no input owns focus", async ({ page }) => {
  await page.goto("/");
  await page.locator("body").click({ position: { x: 4, y: 4 } });
  await page.keyboard.press("s");
  await expect(page.getByTestId("settings-screen")).toBeVisible();
});

test("`s` keyed inside the search field does NOT open settings", async ({ page }) => {
  await page.goto("/");
  await page.getByLabel("Search packages").pressSequentially("s");
  await expect(page.getByTestId("settings-screen")).toHaveCount(0);
});

test("the filter box narrows rows; clearing restores", async ({ page }) => {
  await openSettingsViaGear(page);
  await page.getByTestId("settings-filter").fill("debounce");
  await expect(rowOf(page, "tui.debounce_ms")).toBeVisible();
  await expect(rowOf(page, "search.limit")).toHaveCount(0);
  await page.getByTestId("settings-filter").fill("");
  await expect(rowOf(page, "search.limit")).toBeVisible();
});

test("writing search.limit flips the row origin to your config", async ({ page }) => {
  await openSettingsViaGear(page);
  await expect(page.getByTestId("origin-search.limit")).toHaveText(/default/);
  const input = rowOf(page, "search.limit").getByLabel("search.limit", { exact: true });
  await input.fill("80");
  await input.press("Enter");
  await expect(page.getByTestId("origin-search.limit")).toHaveText(/your config/);
  // the write is engine-routed: the reset affordance now offers the default
  await expect(page.getByTestId("reset-search.limit")).toBeVisible();
});

test("the numeric engine clamp badge accompanies clamped keys", async ({ page }) => {
  await openSettingsViaGear(page);
  await expect(rowOf(page, "search.timeout_secs")).toContainText("3..=30");
});

test("danger rows stay visible with their risk sentence", async ({ page }) => {
  await openSettingsViaGear(page);
  await expect(rowOf(page, "behavior.readonly")).toContainText(
    /readonly=true blocks EVERY installing\/removing/,
  );
});

test("a conflict refusal raises the reload banner; reload clears it", async ({ page }) => {
  await openSettingsViaGear(page);
  const input = rowOf(page, "search.limit").getByLabel("search.limit", { exact: true });
  await input.fill("conflict-please");
  await input.press("Enter");
  const banner = page.getByTestId("settings-conflict");
  await expect(banner).toContainText(/config changed on disk/);
  await expect(rowOf(page, "search.limit")).toContainText(/changed on disk/);
  await banner.getByRole("button", { name: "Reload now" }).click();
  await expect(banner).toHaveCount(0);
});

test("readonly write re-arms the flags: main-screen actions disable", async ({ page }) => {
  await page.goto("/");
  // open detail first so the disabled-actions surface is visible after close
  await page.getByLabel("Search packages").fill("firefox");
  await page.getByLabel("Search packages").press("Enter");
  await expect(page.getByRole("status").first()).toContainText(/Found/, { timeout: 3_000 });
  await page
    .getByRole("option")
    .filter({ hasText: /Mozilla Firefox web browser/ })
    .first()
    .click();
  const pane = page.getByRole("complementary", { name: "Package detail" });
  await expect(pane).toBeVisible();

  // toggle the danger switch inside settings
  await page.getByTestId("open-settings").click();
  const sw = rowOf(page, "behavior.readonly").getByRole("switch", { name: /behavior.readonly/ });
  await expect(sw).toHaveAttribute("aria-checked", "false");
  await sw.click();
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await expect(page.getByTestId("origin-behavior.readonly")).toHaveText(/your config/);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("settings-screen")).toHaveCount(0);

  // the refetched flags reached the detail pane (App onChanged seam)
  await expect(pane.getByText(/readonly mode — actions disabled/)).toBeVisible();
});

test("with a D1 theme override, the settings row pins and the toggle is disabled", async ({
  page,
}) => {
  await page.addInitScript(() => {
    // the backend's initialization_script injected value
    (window as { __PAKMAP_THEME_OVERRIDE?: string }).__PAKMAP_THEME_OVERRIDE = "dark";
  });
  await openSettingsViaGear(page);
  const row = rowOf(page, "gui.theme");
  await expect(row).toContainText("pinned to dark by your config");
  await expect(row).toContainText(/takes effect at relaunch/);
  // the header toggle buttons are inert while the override exists
  const light = page.getByRole("radio", { name: "Light theme" });
  await expect(light).toBeDisabled();
  // unpinning writes `device` via the engine but the pin persists until relaunch
  await row.getByTestId("theme-unpin").click();
  await expect(row).toContainText("pinned to dark by your config");
  expect(
    await page.evaluate(
      () => (window as { __PAKMAP_THEME_OVERRIDE?: string }).__PAKMAP_THEME_OVERRIDE,
    ),
  ).toBe("dark");
});

test("env knobs render as read-only info, and the user path is copyable", async ({ page }) => {
  await openSettingsViaGear(page);
  await expect(page.getByText("PAKMAP_NOCONFIRM")).toBeVisible();
  await expect(page.getByText("/home/mock/.config/pakmap/config.toml")).toBeVisible();
});
