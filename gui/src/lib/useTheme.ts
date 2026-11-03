// useTheme — the GUI theme is a DEVICE preference (localStorage), not a repo
// config: theme.toml stays the TUI's per-terminal override system (ratatui
// style strings). Modes are system | light | dark; "system" follows
// prefers-color-scheme live. index.html carries the same resolution as an
// inline bootstrap so the first paint already matches (no FOUC).
// D1 (v0.10.0): a backend override (window.__PAKMAP_THEME_OVERRIDE from
// [gui] theme) LOCKS the device preference — mode mirrors the override and
// setMode becomes a no-op; the settings screen is the only editor then.
import { useCallback, useEffect, useState } from "react";

export type ThemeMode = "system" | "light" | "dark";
export type Resolved = "light" | "dark";

const KEY = "pakmap-theme";

/** Backend-injected value ([gui] theme ≠ device); absent when device. */
export function themeOverride(): ThemeMode | null {
  const ov = (window as { __PAKMAP_THEME_OVERRIDE?: unknown }).__PAKMAP_THEME_OVERRIDE;
  return ov === "system" || ov === "light" || ov === "dark" ? ov : null;
}

export function systemPrefersLight(): boolean {
  return typeof window !== "undefined" &&
    window.matchMedia !== undefined &&
    window.matchMedia("(prefers-color-scheme: light)").matches;
}

export function readStoredMode(): ThemeMode {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw === "light" || raw === "dark" || raw === "system") return raw;
  } catch {
    /* storage unavailable (e.g. restrictive webview) → system */
  }
  return "system";
}

export function resolveTheme(mode: ThemeMode): Resolved {
  if (mode !== "system") return mode;
  return systemPrefersLight() ? "light" : "dark";
}

export function applyResolved(resolved: Resolved, animate = false): void {
  const root = document.documentElement;
  if (animate && !root.classList.contains("theme-anim")) {
    // theme-switch smoothing only — never on first paint (would animate the
    // whole body from its initial state; that was a real e2e catch)
    root.classList.add("theme-anim");
    window.setTimeout(() => root.classList.remove("theme-anim"), 240);
  }
  root.dataset.theme = resolved;
}

interface MediaQueryListLike {
  addEventListener: (type: string, cb: () => void) => void;
  removeEventListener: (type: string, cb: () => void) => void;
}

function schemeMedia(): MediaQueryListLike | null {
  return window.matchMedia !== undefined
    ? window.matchMedia("(prefers-color-scheme: light)")
    : null;
}

export function useTheme(): {
  /** stored preference — "system" until the user pins something (locked to
   * the override when the config pins one) */
  mode: ThemeMode;
  /** what the pixels show right now */
  resolved: Resolved;
  setMode: (mode: ThemeMode) => void;
  /** true when [gui] theme pins the palette — the toggle renders disabled */
  locked: boolean;
} {
  // The override is written by the init script BEFORE any JS runs and cannot
  // legitimately change mid-session — a plain render-read, no state needed.
  const override = themeOverride();
  const [mode, setModeState] = useState<ThemeMode>(() =>
    override ?? readStoredMode(),
  );
  const [resolved, setResolved] = useState<Resolved>(() =>
    resolveTheme(override ?? readStoredMode()),
  );

  const sync = useCallback((m: ThemeMode, animate: boolean) => {
    const r = resolveTheme(m);
    setResolved(r);
    applyResolved(r, animate);
  }, []);

  // follow the OS while in system mode (no stored pin)
  useEffect(() => {
    if (mode !== "system") return;
    const mq = schemeMedia();
    // initial mount: settle without animating (boot already did)
    sync(mode, false);
    if (mq === null) return;
    const cb = () => sync("system", true);
    mq.addEventListener("change", cb);
    return () => mq.removeEventListener("change", cb);
  }, [mode, sync]);

  // re-sync after any external change (another tab, storage wipe)
  useEffect(() => {
    sync(mode, false);
  }, [mode, sync]);

  const setMode = useCallback(
    (m: ThemeMode) => {
      if (themeOverride() !== null) return; // config override owns the palette
      try {
        localStorage.setItem(KEY, m);
      } catch {
        /* storage unavailable — session-only preference */
      }
      setModeState(m);
      // user-initiated switches get the smoothing transition
      applyResolved(resolveTheme(m), true);
    },
    [],
  );

  return { mode, resolved, setMode, locked: override !== null };
}
