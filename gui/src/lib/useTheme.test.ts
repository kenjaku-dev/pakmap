// @vitest-environment jsdom
// useTheme — device-level theme preference: boot resolution (stored pin beats
// OS), dataset application, and the live system-follow listener.
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  applyResolved,
  readStoredMode,
  resolveTheme,
  useTheme,
  type ThemeMode,
} from "./useTheme";

type MediaStub = { matches: boolean };
let light: MediaStub = { matches: false };

function installMatchMedia(): void {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: (query: string) => {
      if (!query.includes("prefers-color-scheme")) {
        return {
          matches: false,
          addEventListener: () => {},
          removeEventListener: () => {},
        };
      }
      return {
        matches: light.matches,
        addEventListener: (_t: string, cb: () => void) => {
          listeners.add(cb);
        },
        removeEventListener: (_t: string, cb: () => void) => {
          listeners.delete(cb);
        },
      };
    },
  });
}

const listeners = new Set<() => void>();

function flipSystem(toLight: boolean): void {
  light = { matches: toLight };
  for (const cb of listeners) cb();
}

function harness(initialMode: ThemeMode | null) {
  if (initialMode === null) localStorage.removeItem("pakmap-theme");
  else localStorage.setItem("pakmap-theme", initialMode);
  return renderHook(() => useTheme());
}

describe("useTheme", () => {
  beforeEach(() => {
    listeners.clear();
    light = { matches: false };
    installMatchMedia();
    localStorage.clear();
    delete document.documentElement.dataset.theme;
  });

  afterEach(() => {
    localStorage.clear();
  });

  it("defaults to system and resolves dark on a dark OS", () => {
    const h = harness(null);
    expect(h.result.current.mode).toBe("system");
    expect(h.result.current.resolved).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(readStoredMode()).toBe("system");
  });

  it("follows a light OS when system-pinned", () => {
    light = { matches: true };
    const h = harness(null);
    expect(h.result.current.resolved).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("a stored pin wins over the OS", () => {
    const h = harness("dark");
    expect(h.result.current.mode).toBe("dark");
    expect(h.result.current.resolved).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("setMode persists and applies", () => {
    const h = harness(null);
    act(() => h.result.current.setMode("light"));
    expect(h.result.current.mode).toBe("light");
    expect(localStorage.getItem("pakmap-theme")).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("re-pinning system rejoins the OS and live-updates on its change", async () => {
    const h = harness("light");
    act(() => h.result.current.setMode("system"));
    expect(h.result.current.resolved).toBe("dark"); // OS starts dark
    await act(async () => flipSystem(true));
    expect(h.result.current.resolved).toBe("light"); // OS change follows through
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("pinned dark is unaffected by OS flips", () => {
    const h = harness("dark");
    flipSystem(true);
    expect(h.result.current.resolved).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("resolveTheme is pure for explicit modes", () => {
    expect(resolveTheme("light")).toBe("light");
    expect(resolveTheme("dark")).toBe("dark");
  });

  it("applyResolved writes the dataset", () => {
    applyResolved("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});

// D1 (v0.10.0): when [gui] theme in the config is ≠ device, the backend
// injects window.__PAKMAP_THEME_OVERRIDE before any JS runs — the hook must
// mirror it, ignore the stored device pin, and refuse setMode (locked=true;
// the settings screen is the only editor then).
describe("useTheme with a config override (D1 lock)", () => {
  const w = window as { __PAKMAP_THEME_OVERRIDE?: ThemeMode };

  function withOverride(mode: ThemeMode | null): void {
    if (mode === null) delete w.__PAKMAP_THEME_OVERRIDE;
    else w.__PAKMAP_THEME_OVERRIDE = mode;
  }

  afterEach(() => {
    withOverride(null);
    localStorage.clear();
  });

  it("an override wins over the stored device pin and reports locked", () => {
    withOverride("dark");
    const h = harness("light"); // stored "light" must NOT win
    expect(h.result.current.mode).toBe("dark");
    expect(h.result.current.resolved).toBe("dark");
    expect(h.result.current.locked).toBe(true);
  });

  it("setMode is a dead end while locked (storage untouched)", () => {
    withOverride("dark");
    const h = harness("light");
    act(() => h.result.current.setMode("system"));
    expect(h.result.current.mode).toBe("dark");
    expect(localStorage.getItem("pakmap-theme")).toBe("light");
  });

  it("override system still follows the OS live", async () => {
    withOverride("system");
    const h = harness("dark");
    expect(h.result.current.resolved).toBe("dark");
    await act(async () => flipSystem(true));
    expect(h.result.current.resolved).toBe("light");
    expect(h.result.current.locked).toBe(true);
  });

  it("a non-enum override value degrades to device (no lock)", () => {
    withOverride("plaid" as ThemeMode); // corrupted injection must degrade
    const h = harness(null);
    expect(h.result.current.mode).toBe("system");
    expect(h.result.current.locked).toBe(false);
  });

  it("no override keeps today's lock-free behavior", () => {
    const h = harness(null);
    expect(h.result.current.locked).toBe(false);
    act(() => h.result.current.setMode("dark"));
    expect(localStorage.getItem("pakmap-theme")).toBe("dark");
  });
});
