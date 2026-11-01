// v0.9.3 Phase 3 — the mock opener mirror must keep the same failure
// envelope as the real Rust gate (pakmap::safety::url → open_package_url):
// every shape the release build rejects, the mock/E2E path rejects too, so
// e2e and unit tests exercise the same UI reactions.
// v0.9.4 Phase 4 — parity is now fixture-driven: BOTH suites consume
// tests/fixtures/url-gate-cases.json (single source). The Rust gate is the
// oracle — a failing case here means fix the mock, never the fixture.
import { describe, expect, it } from "vitest";
import { openExternal } from "./mockIpc";
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
