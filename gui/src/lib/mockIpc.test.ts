// v0.9.3 Phase 3 — the mock opener mirror must keep the same failure
// envelope as the real Rust gate (pakmap::safety::url → open_package_url):
// every shape the release build rejects, the mock/E2E path rejects too, so
// e2e and unit tests exercise the same UI reactions.
import { describe, expect, it } from "vitest";
import { openExternal } from "./mockIpc";

describe("mockIpc openExternal mirrors the Rust URL gate", () => {
  it("accepts real https pages and https news links", async () => {
    await expect(
      openExternal("https://archlinux.org/news/regular-update-round/", "Regular update round"),
    ).resolves.toBeUndefined();
    await expect(openExternal("https://aur.archlinux.org/packages/htop", "htop")).resolves.toBeUndefined();
  });

  it("rejects actionable-scheme and credential shapes with invalid_url", async () => {
    for (const bad of [
      "javascript:alert(1)",
      "data:text/html,<script>x</script>",
      "file:///etc/passwd",
      "http://evil.example.com/phish",
      "https://user:pass@github.com/x",
      "https://a\u{7}.b/",
      "",
      "x".repeat(2049),
      "not a url at all",
    ]) {
      await expect(openExternal(bad, "audit")).rejects.toMatchObject({ code: "invalid_url" });
    }
  });

  it("keeps the loopback-dev exception from the real gate", async () => {
    await expect(
      openExternal("http://127.0.0.1:9/allowed-loopback", "dev target"),
    ).resolves.toBeUndefined();
    await expect(openExternal("http://localhost:8080/", "dev target")).resolves.toBeUndefined();
  });
});
