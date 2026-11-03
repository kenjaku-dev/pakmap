// @vitest-environment jsdom
// SettingsScreen — the "row rendering" half of the plan §3 test contract:
// row anatomy (key/desc/origin chip/reset/apply tag), danger copy visibility,
// clamp badge, filter box wiring, and the D1 theme-lock row rendering.
// The controller is hand-stubbed (its behavior lives in useSettings.test.ts).
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsScreen } from "./SettingsScreen";
import { themeOverride, type ThemeMode } from "../lib/useTheme";
import type { SettingsController } from "../lib/useSettings";
import type { EffectiveConfig, SettingsEntry, SettingsTarget } from "../types";

function makeEntry(key: string, value: string, over: Partial<SettingsEntry> = {}): SettingsEntry {
  return {
    key,
    value,
    default: "50",
    kind: "usize",
    options: [],
    group: "Search",
    danger: "",
    surfaces: { cli: true, tui: true, gui: true },
    apply: "immediate",
    desc: `desc for ${key}`,
    origin: { label: "default", path: null },
    warning: null,
    valid: true,
    clamp: null,
    ...over,
  };
}

const DATA: EffectiveConfig = {
  app_version: "0.10.0-test",
  entries: [
    makeEntry("search.limit", "50", { default: "50", value: "50" }),
    makeEntry("search.source", "all", {
      kind: "enum",
      default: "all",
      options: [
        { name: "all", desc: "official repos + AUR + flatpak" },
        { name: "aur", desc: "AUR only" },
      ],
    }),
    makeEntry("search.timeout_secs", "15", { default: "15", value: "15", clamp: [3, 30] }),
    makeEntry("tui.debounce_ms", "400", {
      group: "Terminal interface",
      default: "400",
      value: "400",
      surfaces: { cli: true, tui: true, gui: false },
    }),
    makeEntry("behavior.readonly", "false", {
      kind: "bool",
      default: "false",
      group: "Behavior & safety",
      danger: "danger",
    }),
    makeEntry("gui.theme", "dark", {
      kind: "enum",
      default: "device",
      value: "dark",
      group: "GUI",
      origin: { label: "user", path: "/home/mock/.config/pakmap/config.toml" },
      options: [
        { name: "device", desc: "the device/webview decides" },
        { name: "system", desc: "follow the system light/dark setting" },
        { name: "light", desc: "force light palette" },
        { name: "dark", desc: "force dark palette" },
      ],
    }),
  ],
  env_rows: [{ name: "PAKMAP_NOCONFIRM", desc: "set to 1 to skip all confirmations" }],
  warnings: ["warn: unknown key 'legacy' — ignored", "note: deprecated alias"],
  user_path: "/home/mock/.config/pakmap/config.toml",
  project_path: null,
  project_trusted: false,
};

function stubController(over: Partial<SettingsController> = {}): SettingsController {
  const data = over.data ?? DATA;
  return {
    data,
    loadError: null,
    query: "",
    setQuery: () => {},
    groupFilter: "All",
    setGroupFilter: () => {},
    groups: ["All", "Search", "Terminal interface", "Behavior & safety", "GUI", "Environment"],
    visible: data.entries,
    target: "user" as SettingsTarget,
    setTarget: () => {},
    rowErrors: {},
    conflict: false,
    reload: () => {},
    set: () => {},
    unset: () => {},
    busyKey: null,
    clearRowError: () => {},
    ...over,
  } as unknown as SettingsController;
}

describe("SettingsScreen", () => {
  afterEach(cleanup);

  it("renders every row: key, desc, origin chip, reset affordance", () => {
    render(<SettingsScreen ctl={stubController()} onClose={() => {}} />);
    expect(screen.getByTestId("setting-row-search.limit")).toBeTruthy();
    expect(screen.getByText("desc for search.limit")).toBeTruthy();
    // search.limit IS at default → no per-row reset
    expect(screen.queryByTestId("reset-search.limit")).toBeNull();
    // gui.theme is user-set → origin chip + reset
    expect(screen.getByTestId("origin-gui.theme").textContent).toContain("your config");
    expect(screen.getByTestId("reset-gui.theme")).toBeTruthy();
  });

  it("the danger row always shows with its risk sentence", () => {
    render(<SettingsScreen ctl={stubController()} onClose={() => {}} />);
    const row = screen.getByTestId("setting-row-behavior.readonly");
    expect(within(row).getByRole("switch")).toBeTruthy();
    expect(row.textContent).toContain("readonly=true blocks EVERY installing/removing");
  });

  it("a numeric row shows the engine clamp badge (plan §3)", () => {
    render(<SettingsScreen ctl={stubController()} onClose={() => {}} />);
    expect(screen.getByText("3..=30")).toBeTruthy();
  });

  it("a terminal-only key is visible and labeled as such", () => {
    render(<SettingsScreen ctl={stubController()} onClose={() => {}} />);
    const row = screen.getByTestId("setting-row-tui.debounce_ms");
    expect(within(row).getByText("terminal-only")).toBeTruthy();
  });

  it("engine warnings render verbatim in the warnings block", () => {
    render(<SettingsScreen ctl={stubController()} onClose={() => {}} />);
    expect(document.body.textContent).toContain("warn: unknown key 'legacy' — ignored");
    expect(document.body.textContent).toContain("note: deprecated alias");
  });

  it("the filter box feeds the controller", () => {
    const setQuery = vi.fn();
    render(<SettingsScreen ctl={stubController({ setQuery })} onClose={() => {}} />);
    fireEvent.change(screen.getByTestId("settings-filter"), { target: { value: "a" } });
    expect(setQuery).toHaveBeenCalled();
  });

  it("env rows and the copyable config path render", () => {
    render(<SettingsScreen ctl={stubController()} onClose={() => {}} />);
    expect(screen.getByText("PAKMAP_NOCONFIRM")).toBeTruthy();
    expect(screen.getByText("/home/mock/.config/pakmap/config.toml")).toBeTruthy();
  });

  it("conflict state renders the reload banner", () => {
    render(<SettingsScreen ctl={stubController({ conflict: true })} onClose={() => {}} />);
    expect(
      screen.getByTestId("settings-conflict").textContent!.includes("config changed on disk"),
    ).toBe(true);
  });

  it("with the D1 override active, gui.theme renders the lock row that unpins via the engine", () => {
    (window as { __PAKMAP_THEME_OVERRIDE?: ThemeMode }).__PAKMAP_THEME_OVERRIDE = "dark";
    const set = vi.fn();
    render(<SettingsScreen ctl={stubController({ set })} onClose={() => {}} />);
    expect(themeOverride()).toBe("dark");
    const row = screen.getByTestId("setting-row-gui.theme");
    expect(row.textContent!.includes("pinned to dark by your config")).toBe(true);
    fireEvent.click(within(row).getByTestId("theme-unpin"));
    expect(set).toHaveBeenCalledWith("gui.theme", "device");
  });
});
