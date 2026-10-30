// Mock gateway — E2E/smoke harness only (Playwright dev-browser mode).
// Scripted staged legs + fixtures that mirror what the real backend sends,
// including supersede semantics (a newer run silences an older one) and the
// same trigger verb "err" that exercises the error card.
//
// Gated by VITE_PAKMAP_MOCK_IPC=1 so the production bundle never includes it.
import type {
  ConfigFlags,
  ConfigSummary,
  GuiError,
  NewsItemDto,
  OpMessage,
  OpRequest,
  PackageDto,
  PkgbuildResult,
  SearchEvent,
  SearchFilters,
  StatsDto,
  UpdatesResult,
} from "../types";
import { normalizeGuiError } from "./tauriGateway";

export type SearchEventSink = (event: SearchEvent) => void;

const FAST = import.meta.env.VITE_PAKMAP_MOCK_FAST === "1";
const STEP = FAST ? 25 : 300;

const SUMMARY: ConfigSummary = {
  app_version: "0.7.0",
  limit: 50,
  source: "all",
  timeout_secs: 30,
  flatpak_enabled: true,
  custom_aur_rpc: false,
  debounce_ms: 50,
};

const pkg = (name: string, repo: string, extra: Partial<PackageDto> = {}): PackageDto => ({
  name,
  version: "1.0-1",
  description: `${name} — mock description for testing`,
  repo,
  arch: "x86_64",
  url: "https://example.org",
  installed: false,
  votes: null,
  popularity: null,
  out_of_date: null,
  maintainer: "someone",
  last_modified: 1600000000,
  depends: null,
  optdepends: null,
  required_by: null,
  optional_for: null,
  reason: null,
  orphan: false,
  ...extra,
});

const LEGS: Record<"repo" | "aur" | "flatpak", PackageDto[]> = {
  repo: [
    pkg("firefox", "extra", { installed: true, description: "Mozilla Firefox web browser" }),
    pkg("firefox-esr", "extra"),
  ],
  aur: [
    pkg("firefox-bin", "aur", { votes: 421, popularity: 3.14, installed: true }),
    pkg("firefox-nightly", "aur", { votes: 42, out_of_date: 1600000000 }),
    pkg("orphaned-thing", "aur", { votes: 3, maintainer: null }),
  ],
  flatpak: [pkg("org.mozilla.firefox", "flatpak", { arch: null, maintainer: null })],
};

const LEGS_NOTHING: Record<"repo" | "aur" | "flatpak", PackageDto[]> = {
  repo: [],
  aur: [],
  flatpak: [],
};

let counter = 0;

function legsFor(query: string): Record<"repo" | "aur" | "flatpak", PackageDto[]> {
  // Fixture selector keeps tests deterministic: "*" → empty everything.
  if (query.startsWith("*")) return LEGS_NOTHING;
  return LEGS;
}

export async function getConfigSummary(): Promise<ConfigSummary> {
  return SUMMARY;
}

export async function startSearch(
  query: string,
  _filters: SearchFilters,
  sink: SearchEventSink,
): Promise<{ generation: number }> {
  const myGen = ++counter;
  const q = query.trim();

  // scripted failure surface — same envelope shape as the real channel errors
  if (q.startsWith("err")) {
    throw {
      code: "repo_search_failed",
      message: `mock failure for '${q}'`,
      hint: "run pacman -Sy (mock)",
    } satisfies GuiError;
  }

  const legs = q.startsWith("orph:") ? LEGS_NOTHING : legsFor(q);

  const send = (stage: "repo" | "aur" | "flatpak" | "done", packages: PackageDto[], delay: number) => {
    setTimeout(() => {
      // Supersede guard: a newer run (counter moved on) silences us — the
      // backend's active_search guard behaves exactly like this.
      if (counter !== myGen) return;
      const event: SearchEvent = { generation: myGen, stage, packages };
      sink(event);
    }, delay);
  };

  // Orphans mode mirrors the local path: carried-payload done.
  if (q.startsWith("orph:")) {
    setTimeout(() => {
      if (counter !== myGen) return;
      sink({
        generation: myGen,
        stage: "done",
        packages: [pkg("some-widget", "local", { installed: true, orphan: true, reason: "dependency" })],
      });
    }, STEP);
  } else {
    send("repo", legs.repo, STEP);
    send("aur", legs.aur, STEP * 2);
    send("flatpak", legs.flatpak, STEP * 3);
    send("done", [], STEP * 4);
  }
  return { generation: myGen };
}

export async function cancelSearch(): Promise<number> {
  counter += 1; // drops all in-flight sends
  return counter;
}

export async function getPackageInfo(name: string): Promise<PackageDto> {
  const hit = LEGS.repo
    .concat(LEGS.aur, LEGS.flatpak)
    .find((p) => p.name === name && p.installed);
  if (!hit) {
    throw {
      code: "package_not_found",
      message: `installed package '${name}' not found in localdb`,
      hint: "the package may not be installed",
    } satisfies GuiError;
  }
  return { ...hit, depends: ["gtk3", "nss"], optdepends: ["langpack"], reason: "explicit", required_by: [] };
}

// ---------------------------------------------------------------------------
// AD operations surface — scripted fixtures with the same semantics the real
// backend guarantees: progress ticks, terminal done exactly once, error codes
// triggerable by op target name ("err-auth" → auth_dismissed etc.), AUR ops
// stream clone/build/install phases.
// ---------------------------------------------------------------------------

const OP_STEP = FAST ? 30 : 200;
const FIRST_OP_STEP = FAST ? 300 : 600;

const UPDATES: UpdatesResult = {
  repo: [
    { name: "firefox", old_version: "139.0-1", new_version: "140.0-2", repo: "extra", out_of_date: false },
    { name: "openssl", old_version: "3.5.1-1", new_version: "3.5.2-1", repo: "core", out_of_date: false },
    { name: "err-auth-sample", old_version: "2.0-1", new_version: "2.1-1", repo: "core", out_of_date: false },
  ],
  aur: [
    { name: "firefox-bin", old_version: "1.0-1", new_version: "1.1-1", repo: "aur", out_of_date: true },
  ],
  repo_err: null,
  aur_err: null,
  orphans: ["orphaned-lib", "some-widget"],
};

const NEWS: NewsItemDto[] = [
  {
    title: "net-leaked-broken requires manual intervention to upgrade",
    link: "https://archlinux.org/news/net-leaked-broken-requires-manual-intervention/",
    pub_date: "Mon, 12 Oct 2026 10:00:00 +0000",
    pub_epoch: 1760256000,
    needs_action: true,
  },
  {
    title: "Regular update round",
    link: "https://archlinux.org/news/regular-update-round/",
    pub_date: "Tue, 06 Oct 2026 08:00:00 +0000",
    pub_epoch: 1759732800,
    needs_action: false,
  },
];

export async function getConfigFlags(): Promise<ConfigFlags> {
  return { readonly: false, flatpak_enabled: true };
}

/** Browser/E2E: no splash windows exist — no-op. */
export function mainWindowReady(): Promise<void> {
  return Promise.resolve();
}

export async function getUpdates(): Promise<UpdatesResult> {
  return UPDATES;
}

export async function getNews(): Promise<NewsItemDto[]> {
  return NEWS;
}

const STATS: StatsDto = {
  explicit: 741,
  dependencies: 1203,
  orphans: 2,
  updates_repo: 3,
  updates_aur: 1,
  updates_flatpak: 0,
  flatpak_installed: 1,
  cache_pacman_bytes: 4_200_000_000,
  cache_pakmap_bytes: 384_000_000,
  cache_pacman_fmt: "4.2 GiB",
  cache_pakmap_fmt: "366.2 MiB",
  // 2 minutes ago — exercises the "just now" branch of the B6 chip.
  aur_checked_at: Math.floor(Date.now() / 1000) - 120,
};

export function getStats(): Promise<StatsDto> {
  return Promise.resolve(STATS);
}

function mockOpErrorChains(request: OpRequest): { code: string; message: string; hint: string } | null {
  const target =
    request.kind === "install_repo" || request.kind === "remove"
      ? request.args.name
      : request.kind === "install_flatpak" || request.kind === "remove_flatpak"
        ? request.args.app_id
        : null;
  if (target === null) return null;
  if (target.startsWith("err-auth"))
    return {
      code: "auth_dismissed",
      message: "authentication was dismissed or failed",
      hint: "policykit denied the request",
    };
  if (target.startsWith("err-lock"))
    return {
      code: "db_locked",
      message: "pacman database is locked",
      hint: "wait for the running pacman to finish",
    };
  if (target.startsWith("err"))
    return {
      code: "op_failed",
      message: `mock op failure for '${target}'`,
      hint: "exit code 1 — see the operation journal",
    };
  return null;
}

/** phase-boundary cancel registry shared per browser page (module scope) */
const canceledSet = new Set<number>();

export async function runOp(
  request: OpRequest,
  sink: (msg: OpMessage) => void,
): Promise<number> {
  const id = ++counter;
  const failure = mockOpErrorChains(request);
  const phases =
    request.kind === "install_aur"
      ? ["clone", "build", "install"]
      : ["prepare", "run"];
  let step = 0;
  const tick = (first: boolean) => {
    if (counter !== id) return; // supersede guard like the real backend
    // honor a phase-boundary cancel exactly like the backend guard: before
    // the next tick (=phase), the op terminates with op_canceled
    if (canceledSet.has(id)) {
      canceledSet.delete(id);
      sink({
        kind: "done",
        op_id: id,
        ok: false,
        exit_code: null,
        error: {
          code: "op_canceled",
          message: "operation canceled before the next phase",
          hint: "a running transaction is never killed mid-flight, by design",
        },
      });
      return;
    }
    if (step < phases.length) {
      sink({
        kind: "progress",
        op_id: id,
        phase: phases[step]!,
        line: `${phases[step] as string}: scripted line ${step + 1}`,
      });
      step += 1;
      // slow first gap (like an open polkit dialog) then rapid ticks —
      // gives the Cancel button a stable click window in the battery
      setTimeout(() => tick(false), first ? FIRST_OP_STEP : OP_STEP);
    } else if (failure !== null) {
      sink({ kind: "done", op_id: id, ok: false, exit_code: 255, error: failure });
    } else {
      sink({ kind: "done", op_id: id, ok: true, exit_code: 0, error: null });
      return;
    }
  };
  setTimeout(() => tick(true), FIRST_OP_STEP);
  return id;
}

export async function cancelOperation(opId: number): Promise<boolean> {
  canceledSet.add(opId);
  return opId <= counter;
}

export async function dropOperation(_opId: number): Promise<void> {
  /* scripted gateway has no registry */
}

/**
 * Mock of the v0.9.3 opener: the release build routes ALL opens through the
 * `open_package_url` Rust command (pakmap::safety::url gate). This mirror
 * keeps the same failure envelope in browser/E2E runs — unsafe URLs reject
 * with `invalid_url`, safe ones resolve (deliberately a no-op: there is no
 * OS browser target here). Keep this in sync with
 * `pakmap::safety::url::validate_external_url`'s policy.
 */
const MOCK_URL_MAX_BYTES = 2048;

// eslint-disable-next-line no-control-regex
const MOCK_URL_CONTROLS = /[\u{0000}-\u{001F}\u{007F}-\u{009F}]/u;

export async function openExternal(url: string, _auditName: string): Promise<void> {
  const bad = (why: string) =>
    // Same envelope the real gateway throws (normalizeGuiError passes it).
    ({ code: "invalid_url", message: `invalid_url: ${why}`, hint: null });
  if (url.length === 0) throw bad("empty URL");
  if (url.length > MOCK_URL_MAX_BYTES) throw bad("URL too long");
  if (MOCK_URL_CONTROLS.test(url)) throw bad("URL contains control characters");
  const m = /^([a-z][a-z0-9+.-]*):/i.exec(url);
  if (!m) throw bad("not a parseable URL");
  const scheme = m[1].toLowerCase();
  // Loopback-only http exception, mirroring `is_loopback_host`.
  const loopback = /^https?:\/\/(localhost|127\.\d+\.\d+\.\d+|\[::1\])([:/?#]|$)/i.test(url);
  const allowed = scheme === "https" || (scheme === "http" && loopback);
  if (!allowed) throw bad(`scheme '${scheme}' is not allowed — only https pages are opened`);
  // userinfo rejection parsed the runt way: strip the authority slashes the
  // WHATWG parser would have consumed (the raw scheme regex only eats "…:").
  const authority = url
    .slice(m[0].length)
    .replace(/^\/+/, "")
    .split(/[/?#]/)[0];
  if (authority.includes("@")) throw bad("URL contains embedded credentials");
}

// ---------------------------------------------------------------------------
// v0.9 features — PKGBUILD review, package-URL opener, tray event seam.
// Fixtures key off the EXISTING LEGS names (no new search rows: the e2e search
// totals stay stable). `err-`-prefixed names exercise the failure envelope,
// same trigger verb as everywhere else.
// ---------------------------------------------------------------------------

/** Minimal realistic PKGBUILD used by the firefox-bin fixture. */
const FIREFOX_BIN_PKGBUILD = [
  "# Maintainer: someone <someone@example.org>",
  "pkgname=firefox-bin",
  "pkgver=113.0",
  "pkgrel=1",
  "pkgdesc='Mozilla Firefox web browser (binary)'",
  "arch=('x86_64')",
  "url='https://www.mozilla.org/firefox/'",
  "license=('MPL')",
  "depends=('gtk3' 'dbus-glib')",
  "conflicts=('firefox')",
  "source=(${pkgname}-${pkgver}.tar.bz2)",
  "package() {",
  "  install -Dm755 firefox ${pkgdir}/usr/bin/firefox",
  "}",
].join("\n");

export async function getPkgbuild(name: string, fetch: boolean | null): Promise<PkgbuildResult> {
  const noneRes = (nm: string): PkgbuildResult => ({
    name: nm,
    source: "none",
    content: null,
    truncated: false,
    diff: null,
    fetch_ok: false,
  });
  if (name.startsWith("err-")) {
    throw {
      code: "pkgbuild_clone_failed",
      message: `git clone/pull failed for 'aur.archlinux.org/${name}.git' (mock)`,
      hint: "check connectivity to aur.archlinux.org and retry",
    } satisfies GuiError;
  }
  if (!fetch) {
    // firefox-bin has a previous clone (with local-diff), firefox-nightly's
    // clone is clean, orphaned-thing was never installed/cloned.
    if (name === "firefox-bin") {
      return {
        name,
        source: "cache",
        content: FIREFOX_BIN_PKGBUILD,
        truncated: false,
        diff: " pkgname=firefox-bin\n- pkgver=112.0\n+ pkgver=113.0\n  pkgrel=1",
        fetch_ok: false,
      };
    }
    if (name === "firefox-nightly") {
      return {
        name,
        source: "cache",
        content: FIREFOX_BIN_PKGBUILD.replace("firefox-bin", "firefox-nightly"),
        truncated: false,
        diff: null,
        fetch_ok: false,
      };
    }
    return noneRes(name);
  }
  // fetch=true: orphaned-thing clones fresh; nightly's refresh FAILS — the
  // backend stays fail-open: cached copy served, fetch_ok=false, source stays
  // "cache" (same shape as git-DNS-offline runs in production).
  if (name === "firefox-nightly") {
    return {
      name,
      source: "cache",
      content: FIREFOX_BIN_PKGBUILD.replace("firefox-bin", "firefox-nightly"),
      truncated: false,
      diff: null,
      fetch_ok: false,
    };
  }
  if (name === "orphaned-thing") {
    return {
      name,
      source: "fetched",
      content: FIREFOX_BIN_PKGBUILD.replace("firefox-bin", "orphaned-thing"),
      truncated: false,
      diff: null,
      fetch_ok: true,
    };
  }
  return { name, source: "cache", content: FIREFOX_BIN_PKGBUILD, truncated: false, diff: null, fetch_ok: true };
}

/** Mock opener: reject nothing, record nothing — validation lives in Rust. */
export async function openPackageUrl(_name: string, _url: string): Promise<void> {}

/** Mock tray seam: no tray in a browser E2E — returns a no-op unlisten. */
export async function onTrayCheckUpdates(_handler: () => void): Promise<() => void> {
  return () => {};
}

export { normalizeGuiError };
