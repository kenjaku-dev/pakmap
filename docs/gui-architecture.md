# pakmap GUI — architecture & IPC contract

Phase AB record (2026-10-17, base `main @ 961baaa`). Decision locked: **Tauri 2.12 shell
over the existing `pakmap` core library** — React 19 + TypeScript + Tailwind 4 frontend,
WebKitGTK system webview, single binary, zero logic duplication.

Visual companion: [`gui-architecture.html`](./gui-architecture.html) (self-contained dark
diagram; PNG/PDF export via the ⋯ menu).

## Spike numbers (decision gate)

| Metric | Result | Gate |
|---|---|---|
| Release binary | **4.1 MB** (`target/release/pakmap-gui`) | ≤ 10 MB budget from the sprint — pass |
| `cargo build --release -p pakmap-gui` (cold) | 4m 01s | acceptable (deps cached after first build) |
| Smoke run `PAKMAP_GUI_SMOKE_MS=4000` | exit 0 **and** frontend IPC registered (boot-assert; pre-AF this proved process liveness only) | pass |
| webkit2gtk-4.1 | 2.54.1 present (pacman) | pass |
| Frontend `tsc --noEmit && vite build` | 31 modules, ~1.4 s, 71 KB gzip JS | pass |

Decision recorded: **Tauri confirmed** (no egui fallback needed). Startup/interaction
budgets (cold start ≤ 1 s interactive; repo leg paints first) are enforced in AF with
real measurements — the smoke harness proves the auto-quit path, not wall-clock budgets.

## Scope (frozen via parity matrix — anything not listed is v0.9)

### In scope (v0.8.0)

- **FR-1 Search parity:** staged streaming search (repo → AUR → flatpak) with the TUI's
  generation-guard semantics; filters: source, aur_by, regex, installed_only,
  orphans_only, limit.
- **FR-2 Inspection:** detail pane — description, version, repo, installed/update badge,
  AUR popularity, flatpak app id.
- **FR-3 Operations:** install / remove / upgrade / clean with pkexec escalation
  (polkit dialog), user-scope flatpak ops, honest failure states.
- **FR-4 Updates view** (repo + AUR), **FR-5 news feed**, **FR-6 orphans + related/
  diff actions**, **FR-7 config + theme reuse** (`config.toml` / `theme.toml` shared
  with CLI/TUI as single source of truth).

### Non-functional requirements

- **NFR-1** Cold start ≤ 1 s to interactive on release; first painted results = repo leg.
- **NFR-2** AUR warm pool fired at app launch (inherits the ~320 ms sprint guarantee).
- **NFR-3** No `unwrap`/`expect` in the GUI crate (same P5 lint contract as core).
- **NFR-4** CI gates: `cargo fmt/clippy` on the workspace, `pnpm -C gui typecheck/build`,
  166-test CLI/TUI contract stays green at every phase.
- **NFR-5** GUI never runs as root; pacman ops via pkexec, flatpak ops user-scope.

### Explicit out of scope (v0.8.0)

Flathub/Flatpak packaging (AUR PKGBUILD only, AE/AF), Windows/macOS, mobile, system tray,
daemon/service mode, i18n, auto-update, per-window multi-search, plugin surface.

### Parity matrix (TUI surface → GUI surface → phase)

| TUI surface | GUI equivalent | Phase |
|---|---|---|
| Search input (fuzzy, debounced) | search field + filter bar | AC |
| Staged per-source results (Repo/Aur/Flatpak) | per-leg stream groups, live totals | AC |
| Detail view | detail pane | AC |
| Install / remove (keybind flows) | row actions + confirm dialog + progress | AD |
| Update view | Updates tab | AD |
| Clean | Clean section | AD |
| News | News tab | AD |
| Orphans / related | sidebar tools | AD |
| Status bar / stats | status strip | AE polish |
| Themes | token-mapped light/dark/system | AE |

## Workspace & process layout

```text
pakmap/                        # workspace root; default-members = ["."] (CLI/TUI gates untouched)
├── src/                       # pakmap lib + CLI + TUI (frozen contract, 166 tests)
├── gui/
│   ├── package.json           # pnpm@12.14.0, vite 7, tauri CLI 2.12 lockstep
│   └── src-tauri/             # pakmap-gui crate (workspace member)
│       ├── src/lib.rs         # commands + event surface (this contract)
│       ├── tauri.conf.json    # identifier io.github.kenjaku-dev.pakmap, bundle off (AF)
│       └── capabilities/      # least-privilege IPC allowlist (core:default today)
└── .github/workflows/ci.yml   # path-filtered gui job (archlinux container)
```

`pakmap-gui` is a path-dependency consumer of `pakmap` (the lib). The CLI/TUI binaries
are untouched; `default-members = ["."]` keeps every existing bare `cargo` invocation
(root crate only) CI-compatible. MSRV is workspace-pinned to **1.90** (Tauri 2.12 policy).
Tauri crate, CLI, and JS API stay pinned in lockstep on 2.12.x; upgrades re-evaluated
after v0.8.0.

## IPC contract (single source of truth for AC/AD work)

Commands are `#[tauri::command]` fn's in `gui/src-tauri/src/lib.rs`, stateless over the
lib (no config duplication — `Config::load()`/lib calls own the truth). **Rust note:
commands must not be `pub fn`** — on rustc ≥ 1.99 the command macro's generated
`pub use {__cmd__*, …}` collides with `#[macro_export]` for public commands (E0255);
keep commands crate-private.

### Shipped (AB spike → AC search MVP; AD ops below)

| Command | Signature | Returns |
|---|---|---|
| `greet` | `{ name: string }` | `string` (IPC liveness probe) |
| `get_config_summary` | — | `ConfigSummary` (incl. `debounce_ms` clamped 50..=5000) |
| `search` | `{ query, filters, onEvent: Channel<SearchEvent> }` | `u64` generation; staged events stream on the channel |
| `cancel_search` | — | `u64` (new generation; in-flight sends drop) |
| `package_info` | `{ name }` | full `PackageDto` (installed rows: deps/reason via localdb) |

`filters` (`SearchFilters`) = `{ source: "all"|"repo"|"aur"|"flatpak", aur_by,
use_regex, installed_only, orphans_only, limit }`. Backend semantics mirror the TUI
orchestrator exactly: `installed_only`/`orphans_only` become the LOCAL path
(`search_local` + `list_orphans` intersection, enriched orphan badges) carried by a
payload `done` event; otherwise three parallel legs join sequentially, each sent the
moment it completes behind the `active_search` generation guard, then an empty `done`.
A one-shot AUR warm (HEAD) overlaps the repo leg.

`ConfigSummary` = `{ app_version, limit, source, timeout_secs, flatpak_enabled,
custom_aur_rpc, debounce_ms }` — resolved-effective config (user file, fail-open
defaults).

Frontend guard model: the hook binds a monotonic client identity into the per-call
event sink, so IPC ordering races cannot mix runs; the backend's own generation guard
prevents wasted sends for superseded searches. Vitest fixtures mirror the TUI's four
staged-flow tests (accumulate→swap, superseded discard, carried Done, fetch reset).

### Shipped (AD — operations)

| Command | Request | Response |
|---|---|---|
| `run_op` | `{ op: OpRequest, onEvent: Channel<OpMessage> }` | `u64` op_id; progress/done stream on the channel |
| `cancel_operation` | `{ opId }` | `bool` — flag is honored at phase boundaries only |
| `drop_operation` | `{ opId }` | frees the cancel-flag registry entry |
| `get_config_flags` | — | `{ readonly, flatpak_enabled }` (actions disable up-front) |
| `get_updates` | — | `UpdatesResult` (both legs degrade independently) |
| `get_news` | — | `NewsItem[]` (RSS subset, `needs_action` flag) |
| `get_stats` | — | `GuiStats` (TUI `--stats` parity — AE) |

`OpRequest` is a tagged union on the wire: `{ "kind": "install_repo", "args":
{ "name" } }`, kinds `install_repo` / `install_flatpak` / `install_aur` / `remove` /
`remove_flatpak` / `remove_orphans` / `upgrade` / `refresh` / `clean { keep }`.

Screen layout of the surface: AUR installs are `clone (bounded) → build (makepkg,
unprivileged, cwd=clone dir) → pkexec pacman -U`; the clone dir matches the
TUI/diff module (`diff::clone_dir_for`); built *.pkg.tar.{zst,xz,gz} artifacts are
globbed after makepkg. Repo ops are single-phase; flatpak ops run user-scope
unprivileged (`--noninteractive`). Post-op, the AUR update memo invalidates
(phase-Z audit rule: stale memos lie).

Replacement of the AB "planned" list: `updates` → `get_updates`; the `list_installed`
command was folded into the AC search local path (installed-only queries already hit
localdb); `stats` shipped in AE as `get_stats`. Mutating ops split into `run_op(kind,…)`
+ events instead of per-kind commands so one cancel-semantics implementation covers
all nine kinds.

### Streaming events (one channel per operation)

Search mirrors the TUI's `SearchStage::{Repo, Aur, Flatpak, Done}` and reuses the same
`active_search` generation guard — superseded searches drop out incrementally, first leg
paints immediately:

```jsonc
// search://stage
{ "generation": 7, "stage": "repo",  "packages": [PackageDto…] }  // then "aur", "flatpak"
{ "generation": 7, "stage": "done",  "packages": [] }             // leg complete
```

Operations (AD) emit `{ kind: "progress"|"done" }` messages on one channel per op
(`indicatif` stays CLI-only; `percent` was dropped — pacman piped output carries no
reliable percentages, phases+lines are the honest signal):

```jsonc
// op messages
{ "kind": "progress", "op_id": 12, "phase": "install", "line": "installing htop-3.4" }
{ "kind": "done",     "op_id": 12, "ok": true, "exit_code": 0, "error": null }
{ "kind": "done",     "op_id": 13, "ok": false, "exit_code": 255,
  "error": { "code": "auth_dismissed", "message": "…", "hint": "…" } }
```

Frontend ops model: a single active operation (`opsReducer`) — progress lines
accumulate into a 100-line journal ring, `done` is terminal, messages for foreign
op ids and post-terminal stragglers are dropped, and dismissing the banner calls
`drop_operation`. One op runs at a time; the Cancel button stays honest with the
copy "cancels before the next phase — the running transaction is never killed
mid-flight".

### AD failure matrix (stderr/exit → `{ code, message, hint }`)

Classified from the bounded stderr tail in `ops.rs` (`classify_op_failure`); keep in
sync with the table when adding codes:

| code | trigger | the card tells the user |
|---|---|---|
| `pkexec_missing` | pkexec not on PATH (pre-flight) | install polkit + a polkit agent |
| `auth_dismissed` | pkexec "disapproved/dismissed/not authorized" | retry and answer the polkit dialog |
| `db_locked` | pacman "database is locked" | wait for the running pacman / stale db.lck |
| `network_failure` | sync/download failures, timeouts | check mirrors and retry |
| `unresolvable_deps` | "could not satisfy dependencies" | resolve the listed deps first |
| `package_not_found` | "target not found" | spelling/repo coverage |
| `readonly_mode` | `behavior.readonly = true` guard | how to turn readonly off |
| `paccache_missing` | paccache not installed | install pacman-contrib |
| `flatpak_missing` | flatpak not installed (pre-flight) | install flatpak or disable the leg |
| `aur_clone_failed` | git clone/pull failure (bounded 300s) | connectivity to aur.archlinux.org |
| `aur_build_empty` | makepkg produced no artifact | read the build journal (bad checksum/PKGBUILD) |
| `invalid_name` | empty/traversal package name | reject banner (guard, pre-flight) |
| `op_canceled` | cancel honored at a phase boundary | next-phase only, by design |
| `op_failed` | anything else | last stderr line + exit code |

### Error envelope

All command errors reject with `{ code, message, hint? }` (`GuiError`). Strings are
mapped from the CLI's `error.rs` hints — one source of truth, no GUI-specific copies.
Seeded codes (AB): `empty_query`, `repo_search_failed`; AC adds `aur_search_failed`,
`flatpak_search_failed`, `config_unreadable`; AD adds the privilege/ops matrix
(`pkexec_missing`, `auth_dismissed`, `db_locked`, `flatpak_missing`, …) — each maps to
an actionable error card with the CLI's hint text.

### Frontend DTO

`PackageDto` (AC): every `Package` field the detail pane needs — `{ name, version,
description, repo, arch, url, installed, votes, popularity, out_of_date, maintainer,
last_modified, depends, optdepends, required_by, optional_for, reason, orphan }`;
list badges (`OOD`/`ORPHAN`/`UNMAINTAINED`) keep `Package::badges()` semantics.
camelCase? — no: Tauri serializes field names as-is (snake_case), the TS types mirror
the Rust structs 1:1.

### Privilege & cancellation model

- GUI process stays unprivileged; pacman ops spawn `pkexec pacman` (polkit owns the
  auth UI); flatpak ops run user-scope. Missing pkexec ⇒ actionable error, never a
  root GUI.
- GUI divergence from the TUI (documented, deliberate): pacman/paccache in the GUI
  run **always non-interactive** (`--noconfirm`) because there is no TTY — the polkit
  dialog replaces pacman's own confirm prompt. AUR builds run makepkg against the
  cloned PKGBUILD (`-srf --noconfirm`); the live PKGBUILD review stays a TUI surface
  (the GUI copies the link instead); AUR dependency resolution inherits pacman -U's
  failure mode (missing AUR deps surface as `unresolvable_deps`).
- Cancellation: searches cancelable anytime (generation guard makes it free); pacman
  transactions never killed mid-transaction — op cancel applies between phases only
  (`OpsState` cancel flags are read at clone→build→install boundaries), surfaced in
  the UI copy, and the terminal event says so (`op_canceled`).
- Arch news guard: intervention items surface as a News-tab banner and per-item
  `[ACTION REQUIRED]` badges; the GUI does not gate the upgrade op on a live news
  fetch (divergence: the TUI prompts; the banner is the discoverable path).
- Links (news, package URLs) copy to the clipboard in AD; a real `tauri-plugin-opener`
  lands with AF packaging (capability addition).

## Theme system, visual identity & stats (AE)

### Theme model

The GUI theme is a **device preference** (webview `localStorage` key
`pakmap-theme`: `system | light | dark`, default `system`), not a repo config —
`theme.toml` stays the TUI's per-terminal override system (ratatui style strings),
and no shared-config fields were added (the frozen 164-test contract is untouched;
`get_stats` composes existing lib APIs only). Inline bootstrap in `index.html`
resolves the mode **before first paint** (no dark-flash on light-system setups);
`data-theme` on `:root` is the CSS driver everything consumes.

Tokens are two-tier (`gui/src/index.css`):

- **primitives** `--pk-*` — one palette slot per value, assigned under
  `:root, :root[data-theme=dark]` (dark = brand default, = the landing
  `design-system/pakmap/MASTER.md` palette) and `:root[data-theme=light]`;
- **semantic** `--color-*` — 1:1 aliases consumed by Tailwind utilities
  (`bg-card`, `text-muted-foreground`, …), so a switch is a variable
  reassignment, not a re-render. No hard-coded colors outside `index.css`
  (the two translucent gradients use `color-mix` against tokens).

The light scheme is an independently designed palette, **not an inversion**: accent
text shades move two steps deeper so text/focus stay ≥ 4.5:1 (`--text-2xs` badges
inherit the same audit):

| Slot | Dark (brand) | Light (derived, AA-verified) |
|---|---|---|
| background / card / card-muted | `#080d17` / `#0f1728` / `#162033` | `#f1f5f9` / `#ffffff` / `#e9eef6` |
| foreground / muted-foreground | `#f1f5f9` / `#94a3b8` | `#0f172a` / `#475569` |
| border | `#2a3a55` | `#cbd6e4` |
| accent (text-safe) | `#22c55e` | `#166534` |
| accent-secondary | `#38bdf8` | `#075985` |
| accent-tertiary | `#a78bfa` | `#6d28d9` |
| destructive | `#ef4444` | `#b91c1c` |
| `*-strong` solid fills | unchanged both themes — white-on-`#7c3aed` / `#b91c1c` is theme-independent |

TUI token correspondence (mapping, not coupling): `--accent` ↔ `theme.toml`
`repo_local`/`repo_extra` (green family), `--accent-tertiary` ↔ `repo_aur`
(magenta → violet light-safe shade), `--destructive` ↔ `border_error` (red
family), `highlight_*` → focus/selection styling.

### Toggle, motion, typography

- `ThemeToggle` (header, every tab): `radiogroup` with roving focus
  (`radiogroup` semantics: system/light/dark, arrow-key roaming, `aria-checked`),
  system option spelled out ("Follow system theme").
- Motion: streaming `row-in` (AC) plus `banner-in`, `modal-in`, skeleton
  shimmer — all keyframes live inside `@media (prefers-reduced-motion:
  no-preference)`; skeletons swap to a static opacity under `reduce`. The 160 ms
  body color transition is `.theme-anim`-scoped to *switches only* — on first
  paint it animated the whole body from its initial state (caught by the new
  e2e token check, hence the assertion reads the `--pk-*` token, not a sampled
  computed color).
- Typography scale gained named dense steps — `--text-2xs` (10 px micro
  badges/chips), `--text-nano` (11 px dense rows/hints) — and the body's 13 px
  stays the deliberate dense-tool exception (rest is Tailwind's ladder). Every
  arbitrary `text-[10–12px]` across components was replaced with the scale
  tokens; none remain.

### Brand & icons

`BrandMark` (`gui/src/components/BrandMark.tsx`) — chunky lowercase "p" whose
bowl carries a map-pin notch, sky "package" dot, on the `#080d17` tile. Same
geometry lives in `gui/src-tauri/icons/pakmap-logo.svg`; the shipped PNG set is
regenerated from it via `rsvg-convert` (commands are in the SVG header comment):
`32x32.png` / `128x128.png` / `128x128@2x.png` / `icon.png` (512) — Windows/mac
bundles remain out of scope (v0.9). The empty state now opens with the mark.

### Stats surface

`get_stats` returns `GuiStats`: the lib's `SysStats` (`--stats` parity: explicit/
dependencies split, orphans, per-source update counts, cache bytes) plus
pre-formatted cache strings via `lib::stats::human_bytes`. AUR count comes from
the existing 5-min update memo, flatpak count from the cached installed list —
no new network work. Rendered as an additive chip row on the Updates tab
(fail-open: a stats failure never blocks the updates list).

## Packaging & release (AF)

### Distribution model (deliberate, no Tauri bundling)

`bundle.active` stays `false` — deb/AppImage packaging is not pursued; the GUI
ships through three channels, all sharing one install layout
(`usr/bin/pakmap-gui`, `usr/share/applications/pakmap-gui.desktop`,
`usr/share/icons/hicolor/{32x32,128x128,256x256}/apps/pakmap-gui.png`):

1. **AUR `pakmap-gui`** — `packaging/aur/pakmap-gui/PKGBUILD` (pretty-sourced
   tag tarball, `pnpm install --frozen-lockfile && pnpm build && cargo build
   --release --locked -p pakmap-gui`; publishing steps in the adjacent README).
   Run deps: `webkit2gtk-4.1 gtk3 polkit pacman git pacman-contrib`;
   `flatpak` optional.
2. **GitHub release tarball** — `pakmap-gui-<v>-x86_64-linux.tar.gz`
   (install-layout tree; `tar xzf -C /`), built by `release.yml`.
3. **Source install** — `make gui-release && make install-gui`
   (`install-gui-assets` is the shared layout step the PKGBUILD mirrors).

`tauri.conf.json` no longer hardcodes `version` — it inherits the crate's
(single-source, like the landing's `sync-version`).

### `tauri-plugin-opener` (AF capability, least privilege)

Registered on the builder; the capability grants **only**
`opener:allow-open-url` — no `open-path` (the GUI never opens filesystem
targets from the frontend). NewsScreen gets a real "Open" action (system
browser via the plugin) while keeping the copy icon as a raw-URL fallback;
the open seam is `ipc.openExternal` (mock gateway no-ops it in E2E).

### CI wiring (gui job + release)

- `ci.yml` gui job now runs the full Playwright battery in the Arch container:
  the `chromium` package is installed as the **library carrier** for the
  downloaded Playwright build (browsers cached via actions/cache), then
  `playwright install chromium && playwright test` (32 specs, mock gateway,
  `reuseExistingServer` disabled in CI).
- `release.yml` builds the GUI alongside the CLI after the verify gate
  (needs `nodejs` + pnpm/action-setup + `webkit2gtk-4.1`) and attaches
  `pakmap-gui-<v>-x86_64-linux.tar.gz` to the GitHub release together with the
  CLI tarball; `generate_release_notes` drafts release notes.

### Runtime budgets (release, measured at v0.8.0)

- Binary size: CLI **6.1M** / GUI **9.0M** with assets embedded (≤ 10M NFR holds;
  ~1.2M of the AB-spike delta is the AF opener plugin pulling zbus, the embed adds
  ~80KB for the dist sidecar; opt-level "z" + lto + strip handle the rest). No
  deb/AppImage bundles, so no further bundling tax.
- Startup: cold self-quit smoke walls **0.59s (MS=300)** – **2.28s (MS=2000)**,
  boot-assert exit 0 (first paint lands inside ~1.1s of process start — webkit2gtk
  init dominating; the timer overshoot walls are the self-quit timer plus
  ~0.2–0.7s graceful teardown). The gate is **content-real** since the AF fix:
  `PAKMAP_GUI_SMOKE_MS` now exits **3** when the frontend never issued IPC within
  the window — a release build without `--features tauri/custom-protocol`
  (dev-mode binary → webview stuck on the devUrl → "Could not connect to
  localhost") cross-validated both ways locally (no-feature=3, feature=0). The
  pre-fix smoke contract "exit 0" was content-blind and shipped exactly that bug
  in the first v0.8.0 GUI tarball (tag re-pointed after the fix; CI's gui job
  additionally runs an xvfb boot smoke on an embedded-asset debug build).
- Memory: main-process peak RSS **~141MB** steady (webkit2gtk webview baseline;
  side webprocesses scale with page content but are far smaller here).

### Final design audit (webdesign-review quick scope: six domains)

- **accessibility** — instrumented: 12 axe scans, zero violations across both
  themes; live regions verified (`StatusStripe`, `OpsBanner` `aria-live=polite`
  without focus theft); dialogs trap and restore focus. Fixed in AF: the news
  copy fallback became a 24×24 icon-button target with a lucide `Copy` glyph
  (target-size + icon-set consistency).
- **usability / ux** — confirm dialogs name the consequence before the action;
  error cards carry the CLI's hint; cancel semantics are spelled out in copy;
  readonly explains itself; empty state teaches the hotkeys. No new findings.
- **ui / typography / color** — semantic two-tier tokens everywhere (no
  hard-coded colors outside `index.css`), named type scale, three accents max,
  AA verified dark and light.
- Recommendation (deferred, v0.9): `DetailPane` URL stays inert text; opening
  package URLs might also deserve the opener affordance.

## Testing strategy (cross-phase)

| Layer | Tooling | Guards |
|---|---|---|
| Core lib | existing 164 tests | frozen contract, untouched |
| Tauri commands | Rust unit tests in `gui/src-tauri` (8: DTO full-field map, stage-name mirror, fail-open parse + AD: failure-matrix classification, keyof wire tagged unions, name guard, makepkg-artifact glob) | this contract |
| Stream reducer | vitest (23: 5 search staged protocol + 10 ops + 8 theme hook) | staged protocol incl. superseded generations (mirrors the 4 TUI staged tests); ops: terminal-once semantics, ring cap, wrong-id guard, canceling copy; theme: boot resolution, pin-beats-OS, live system-follow listener |
| E2E smoke | Playwright dev-browser battery (`pnpm e2e`), 32 specs | search: empty → streaming → done → detail, keyboard-complete, generation guard, error envelope, no-results, overlay below 1020px; AD: tablist semantics, updates both-legs render, confirm → progress → done → dismiss, cancel → `op_canceled`, classified failure card, news intervention banner; AE: theme boot/toggle/persistence/system-follow + light-token check (`e2e/theme.spec.ts`, with `emulateMedia`); axe scans: empty/done/detail/error/dialog + updates/news/confirm-dialog/failed-banner states | **gated in CI since AF** (Arch container: system chromium as lib carrier, cached browser) |
| Accessibility | axe-core scans (`e2e/a11y.spec.ts`) — 12 scans: 9 key states in the dark brand default **+ 3 light-scheme scans** (empty, list+detail, failed banner) | zero violations (wcag 2.1 A/AA); violations fail the battery; scans pin the theme explicitly (Playwright's default color scheme is light — an unpinned boot would silently flip semantics) |

## CI integration

Path filter `gui/**` gates a `gui` job (archlinux container: webkit2gtk-4.1, librsvg,
chromium, node 22 + pnpm): frontend install/build (frozen lockfile), eslint, vitest,
the **full Playwright battery** (AF), `cargo clippy -p pakmap-gui --locked
--all-targets -- -D warnings`, `cargo check -p pakmap-gui --locked`.
`cargo fmt --all` runs in the rust job and covers the GUI crate's Rust files.
Tag pushes (`v*`) trigger `release.yml`: verify → CLI tarball **+ GUI tarball**
(AF) → GitHub release. The `ci-ok` aggregate job requires all triggered jobs green.
