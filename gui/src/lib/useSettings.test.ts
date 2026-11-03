// @vitest-environment jsdom
// useSettings — the settings controller (plan §3 "store" test contract):
// layered origin patch on set/unset, group + filter derivation, conflict and
// per-row failure states, and the onChanged re-arm seam. `./ipc` is mocked —
// the controller's contract is what's under test, not the transport.
import { renderHook, act, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EffectiveConfig } from "../types";
import { useSettings } from "./useSettings";

const entry = (
  key: string,
  value: string,
  overrides: Partial<EffectiveConfig["entries"][number]> = {},
): EffectiveConfig["entries"][number] => ({
  key,
  value,
  default: "10",
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
  ...overrides,
});

const FIXTURE: EffectiveConfig = {
  app_version: "0.10.0-test",
  entries: [
    entry("search.limit", "50", { default: "50", value: "50" }),
    entry("tui.debounce_ms", "400", { group: "Terminal interface", kind: "u64" }),
    entry("behavior.readonly", "false", {
      kind: "bool",
      group: "Behavior & safety",
      danger: "danger",
    }),
  ],
  env_rows: [{ name: "PAKMAP_NOCONFIRM", desc: "skip confirmations" }],
  warnings: ["warn: unknown key 'legacy' in config.toml:3 — ignored"],
  user_path: "/home/mock/.config/pakmap/config.toml",
  project_path: null,
  project_trusted: false,
};

const mocks = vi.hoisted(() => ({
  set: vi.fn<(key: string, value: string, target: string, create: boolean) => Promise<void>>(),
  unset: vi.fn<(key: string, target: string) => Promise<void>>(),
}));

let fixture: EffectiveConfig;

vi.mock("./ipc", () => ({
  getEffectiveConfig: () => Promise.resolve(fixture),
  setConfigEntry: (key: string, value: string, target: string, create: boolean) =>
    mocks.set(key, value, target, create),
  unsetConfigEntry: (key: string, target: string) => mocks.unset(key, target),
}));

function controller(onChanged?: () => void) {
  return renderHook(() => useSettings(onChanged));
}

describe("useSettings", () => {
  beforeEach(() => {
    fixture = structuredClone(FIXTURE);
    mocks.set.mockReset().mockResolvedValue(undefined);
    mocks.unset.mockReset().mockResolvedValue(undefined);
  });

  it("loads the engine fixture and derives groups in engine order", async () => {
    const h = controller();
    await waitFor(() => expect(h.result.current.data).not.toBeNull());
    expect(h.result.current.groups).toEqual([
      "All",
      "Search",
      "Terminal interface",
      "Behavior & safety",
      "Environment",
    ]);
    expect(h.result.current.conflict).toBe(false);
  });

  it("query filters by key, desc and group", async () => {
    const h = controller();
    await waitFor(() => expect(h.result.current.data).not.toBeNull());
    act(() => h.result.current.setQuery("debounce"));
    expect(h.result.current.visible.map((e) => e.key)).toEqual(["tui.debounce_ms"]);
    act(() => h.result.current.setQuery("safety"));
    expect(h.result.current.visible.map((e) => e.key)).toEqual(["behavior.readonly"]);
    act(() => h.result.current.setQuery("zzz-no-match"));
    expect(h.result.current.visible).toEqual([]);
  });

  it("set patches the row: origin flips to the write target, onChanged re-arms", async () => {
    const onChanged = vi.fn();
    const h = controller(onChanged);
    await waitFor(() => expect(h.result.current.data).not.toBeNull());
    await act(async () => h.result.current.set("search.limit", "80"));
    expect(mocks.set).toHaveBeenCalledWith("search.limit", "80", "user", false);
    const row = h.result.current.data?.entries.find((e) => e.key === "search.limit");
    expect(row?.value).toBe("80");
    expect(row?.origin.label).toBe("user");
    expect(row?.origin.path).toBe("/home/mock/.config/pakmap/config.toml");
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it("unset restores default + default origin", async () => {
    fixture.entries[0].origin = { label: "user", path: fixture.user_path };
    const h = controller();
    await waitFor(() => expect(h.result.current.data).not.toBeNull());
    await act(async () => h.result.current.unset("search.limit"));
    expect(mocks.unset).toHaveBeenCalledWith("search.limit", "user");
    const row = h.result.current.data?.entries.find((e) => e.key === "search.limit");
    expect(row?.origin.label).toBe("default");
  });

  it("a conflict refusal lands in rowErrors + the banner state and clears on reload", async () => {
    const h = controller();
    await waitFor(() => expect(h.result.current.data).not.toBeNull());
    mocks.set.mockRejectedValueOnce({
      code: "conflict",
      message: "changed on disk",
      hint: "reload",
    });
    await act(async () => h.result.current.set("search.limit", "80"));
    expect(h.result.current.conflict).toBe(true);
    expect(h.result.current.rowErrors["search.limit"]?.conflict).toBe(true);
    await act(async () => h.result.current.reload());
    expect(h.result.current.conflict).toBe(false);
    expect(h.result.current.rowErrors).toEqual({});
  });

  it("a missing-user-file policy refusal offers the create affordance", async () => {
    const h = controller();
    await waitFor(() => expect(h.result.current.data).not.toBeNull());
    mocks.set.mockRejectedValueOnce({
      code: "policy-refused",
      message: "refusing to write /home/mock/.config/pakmap/config.toml — the file does not exist",
      hint: null,
    });
    await act(async () => h.result.current.set("search.limit", "80"));
    expect(h.result.current.rowErrors["search.limit"]?.canCreateUser).toBe(true);
  });

  it("a project target with no project file demotes the switch back to user on reload", async () => {
    const h = controller();
    await waitFor(() => expect(h.result.current.data).not.toBeNull());
    act(() => h.result.current.setTarget("project"));
    await act(async () => h.result.current.reload()); // fixture has project_path: null
    expect(h.result.current.target).toBe("user");
  });
});
