// AE theme system e2e — boot resolution, UI toggle, persistence, system
// follow. The bootstrap script in index.html resolves BEFORE first paint,
// so assertions read documentElement.dataset.theme (what the pixels show).
import { expect, test } from "@playwright/test";

async function dataTheme(page: import("@playwright/test").Page): Promise<string | undefined> {
  return page.evaluate(() => document.documentElement.dataset.theme);
}

test("no pin + dark OS → dark (brand default) through the toggle UI", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/");
  expect(await dataTheme(page)).toBe("dark");
  await page.getByLabel("Theme").getByRole("radio", { name: "Light theme" }).click();
  expect(await dataTheme(page)).toBe("light");
  // aria state tracks the selection
  await expect(page.getByRole("radio", { name: "Light theme" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
});

test("system follow: light OS flips the scheme without a pin", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/");
  expect(await dataTheme(page)).toBe("light");
  // re-pin system after a light pin: back to following the OS
  await page.getByLabel("Theme").getByRole("radio", { name: "Follow system theme" }).click();
  expect(await dataTheme(page)).toBe("light");
});

test("a stored pin survives reload and beats the OS", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.addInitScript(() => window.localStorage.setItem("pakmap-theme", "dark"));
  await page.goto("/");
  expect(await dataTheme(page)).toBe("dark");
  await page.reload();
  expect(await dataTheme(page)).toBe("dark");
});

test("light theme paints the light palette (token check)", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/");
  const bg = await page.evaluate(() =>
    getComputedStyle(document.documentElement)
      .getPropertyValue("--pk-background")
      .trim(),
  );
  // the light background slot itself (computed colours animate for 160ms on
  // switches, so assert the token — the e2e caught that exact race)
  expect(bg).toBe("#f1f5f9");
});

// v0.10.0 D1 (plan §3): a backend [gui] theme override arrives BEFORE any JS
// runs as window.__PAKMAP_THEME_OVERRIDE — the same inline bootstrap consults
// it first. Mocked here by an init script (dev/E2E has no backend injection).
test("config override dark: boot paints dark even over a stored light pin", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.addInitScript(() => {
    window.localStorage.setItem("pakmap-theme", "light");
    (window as { __PAKMAP_THEME_OVERRIDE?: string }).__PAKMAP_THEME_OVERRIDE = "dark";
  });
  await page.goto("/");
  expect(await dataTheme(page)).toBe("dark");
});

test("config override system: boot follows the OS", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.addInitScript(() => {
    (window as { __PAKMAP_THEME_OVERRIDE?: string }).__PAKMAP_THEME_OVERRIDE = "system";
  });
  await page.goto("/");
  expect(await dataTheme(page)).toBe("light");
});

test("config override active: the header toggle renders disabled with the lock reason", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.addInitScript(() => {
    (window as { __PAKMAP_THEME_OVERRIDE?: string }).__PAKMAP_THEME_OVERRIDE = "dark";
  });
  await page.goto("/");
  const group = page.getByRole("radiogroup", { name: "Theme" });
  await expect(group).toHaveAttribute("aria-describedby", "theme-lock-reason");
  for (const name of ["Follow system theme", "Light theme", "Dark theme"]) {
    await expect(group.getByRole("radio", { name })).toBeDisabled();
  }
});
