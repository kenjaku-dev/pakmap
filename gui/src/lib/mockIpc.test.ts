// v0.9.3 Phase 3 — the mock opener mirror must keep the same failure
// envelope as the real Rust gate (pakmap::safety::url → open_package_url):
// every shape the release build rejects, the mock/E2E path rejects too, so
// e2e and unit tests exercise the same UI reactions.
// v0.9.4 Phase 4 — parity is now fixture-driven: BOTH suites consume
// gui/src/lib/urlGateCases.json (single source; the Rust suite reads it via
// include_str!). The Rust gate is the oracle — a failing case here means
// fix the mock, never the fixture.
import { describe, expect, it, afterEach } from "vitest";
import {
  getEffectiveConfig,
  openExternal,
  setConfigEntry,
  unsetConfigEntry,
} from "./mockIpc";
import gateCases from "./urlGateCases.json";

const fixture = gateCases as {
  version: number;
  cases: { url: string; expect: "accept" | "reject"; why?: string }[];
};

describe("mockIpc openExternal mirrors the Rust URL gate", () => {
  it("accepts real https pages and https news links", async () => {
    await expect(
      openExternal("https://archlinux.org/news/regular-update-round/", "Regular update round"),
    ).resolves.toBeUndefined();
    await expect(openExternal("https://aur.archlinux.org/packages/htop", "htop")).resolves.toBeUndefined();
  });

  it("matches the shared fixture case for case", async () => {
    expect(fixture.version).toBe(1);
    expect(fixture.cases.length).toBeGreaterThanOrEqual(25);
    for (const c of fixture.cases) {
      if (c.expect === "accept") {
        await expect(openExternal(c.url, c.why ?? "audit")).resolves.toBeUndefined();
      } else {
        await expect(openExternal(c.url, c.why ?? "audit")).rejects.toMatchObject({ code: "invalid_url" });
      }
    }
  });

  it("keeps the loopback-dev exception from the real gate", async () => {
    await expect(
      openExternal("http://127.0.0.1:9/allowed-loopback", "dev target"),
    ).resolves.toBeUndefined();
    await expect(openExternal("http://localhost:8080/", "dev target")).resolves.toBeUndefined();
  });
});

// v0.10.0 §3 — the mock settings store flips origins exactly like the engine
// (set → user origin, unset → default) and mirrors the refusal taxonomy so
// E2E drives the same row-failure UI the real backend produces.
describe("mockIpc settings store mirrors engine semantics", () => {
  const REAL_SET = setConfigEntry;
  const REAL_UNSET = unsetConfigEntry;
  const REAL_READ = getEffectiveConfig;

  afterEach(async () => {
    // each planned write mutates the shared mock store; restore the pristine
    // module state so other suites (OpenScreen etc.) meet default origins
    await REAL_UNSET("search.limit", "user");
    await REAL_UNSET("search.source", "user");
    await REAL_UNSET("tui.debounce_ms", "user");
  });

  it("set flips the origin to user; unset returns it to default", async () => {
    await REAL_SET("search.limit", "80", "user", false);
    const after = await REAL_READ();
    const row = after.entries.find((e) => e.key === "search.limit");
    expect(row?.value).toBe("80");
    expect(row?.origin.label).toBe("user");
    expect(row?.origin.path).not.toBeNull();

    await REAL_UNSET("search.limit", "user");
    const reset = await REAL_READ();
    const back = reset.entries.find((e) => e.key === "search.limit");
    expect(back?.value).toBe("50");
    expect(back?.origin.label).toBe("default");
  });

  it("refuses unknown keys and out-of-kind values like the engine", async () => {
    await expect(REAL_SET("search.limt", "80", "user", false)).rejects.toMatchObject({
      code: "unknown-key",
    });
    await expect(REAL_SET("search.limit", "eighty", "user", false)).rejects.toMatchObject({
      code: "invalid-value",
    });
    await expect(REAL_SET("search.source", "sideways", "user", false)).rejects.toMatchObject({
      code: "invalid-value",
    });
    await expect(REAL_SET("gui.theme", "plaid", "user", false)).rejects.toMatchObject({
      code: "invalid-value",
    });
  });

  it("mirrors the conflict / policy / unparseable trigger verbs for E2E", async () => {
    await expect(REAL_SET("search.limit", "conflict-please", "user", false)).rejects.toMatchObject({
      code: "conflict",
    });
    await expect(REAL_SET("search.limit", "policy-please", "user", false)).rejects.toMatchObject({
      code: "policy-refused",
    });
    await expect(REAL_SET("search.limit", "unparseable-please", "user", false)).rejects.toMatchObject({
      code: "unparseable",
    });
  });

  it("project writes are refused without a project file (E2E has none)", async () => {
    await expect(REAL_SET("search.limit", "9", "project", false)).rejects.toMatchObject({
      code: "policy-refused",
    });
  });
});
