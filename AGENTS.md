# pakmap — agent notes

## What lives where

- `src/` + `tests/` — crate **`pakmap`**: CLI (clap) + TUI (ratatui) + the core lib (search/install/updates/stats). Links `libalpm` — **Arch-only builds** (needs pacman 5.1+ / libalpm 16; assume the dev box is Arch).
- `gui/src-tauri/` — crate **`pakmap-gui`**: Tauri 2 backend, consumes `pakmap` as a library dep (`use pakmap::...`). Commands are crate-private `fn`s — this is deliberate: local rustc 1.99 + the Tauri macro collide on `pub async fn` commands (E0255). Register every command in the `generate_handler![...]` list.
- `gui/` — frontend: React + Vite + Tailwind, own pnpm install; `gui/e2e/` = Playwright (35 specs, mock-gateway).
- `pakmap-landing/` — Next.js static marketing site, **separate pnpm install**, has its own auto-generated `AGENTS.md` about Next-version quirks.
- `packaging/aur/` — AUR PKGBUILDs (publish steps in the adjacent README).
- `docs/gui-architecture.md` — **source of truth for the IPC contract**, failure matrix, test truth table, runtime budgets. `context.md` is the append-only log of completed build phases — append an entry there per finished phase.

## Environment quirks

- pnpm is at `~/.local/bin` — `export PATH="$HOME/.local/bin:$PATH"` in every new shell or pnpm "doesn't exist".
- MSRV is pinned workspace-wide (`rust-version = 1.90` in `Cargo.toml`); local toolchain is far ahead (rustc 1.99).
- **Build acceleration lives in the user-global `~/.cargo/config.toml`** — `clang` as linker, `-fuse-ld=mold`, `target-cpu=native`, `rustc-wrapper = "sccache"`. The repo ships **no** `.cargo/config.toml` and CI containers install none of these: never commit linker/toolchain/wrapper config — it would break CI. sccache is user-local only; CI keeps `Swatinem/rust-cache` on `target/`.
- `[profile.dev]` ships `debug = "line-tables-only"` + `split-debuginfo = "unpacked"` (tracked, rustc-feature-only, CI-safe): slim debuginfo → faster debug links across both crates.
- `gui/` and `pakmap-landing/` each need their own `pnpm install --frozen-lockfile` (CI does `pnpm --dir gui ...`).
- Live integration tests (`tests/integration_search`, `make test`) need `sudo pacman -Sy` first; **`PAKMAP_SKIP_LIVE=1 cargo test --locked --lib` gives fully offline unit tests** (exactly how CI's rust job runs them).
- Do not stage or delete `slide/` — untracked, user-owned material.

## Verification (full gate battery; CI mirrors it)

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked        # 183 tests (164 CLI/lib + 19 GUI crate)
(cd gui && pnpm typecheck && pnpm lint && pnpm test && pnpm build)   # vitest: 23
(cd gui && pnpm exec playwright test)
(cd pakmap-landing && pnpm sync-version && pnpm typecheck && pnpm lint && pnpm build)
(env PATH="$HOME/.local/bin:$PATH" make gui-release   # release GUI build with embedded assets; smoke separately, see "Boot smoke contract")
```
```
xvfb-run -a dbus-run-session -- env PAKMAP_GUI_SMOKE_MS=9000 ./target/release/pakmap-gui   # exit 0 expected
```

Order matters: fmt → clippy → tests; run e2e last. Run a single GUI test with `pnpm --dir gui exec playwright test e2e/theme.spec.ts`; single Rust test via `cargo -p` / `--test <name>` as usual.

The fast edit loop is `cargo check` / `cargo clippy` (no codegen) — reserve `cargo build` for when a binary is actually needed (smoke, install, GUI release). Measure phases with `make timings` (report in `target/cargo-timings/cargo-timing.html`). Installed user-level (`~/.local/bin`, via cargo — no sudo): `bacon` (save→check watch loop, `bacon --headless check` for non-TTY) and `cargo-nextest` (parallel tests — `cargo test` stays the CI command so results stay comparable).

## Non-obvious traps

- **Two binaries in the workspace**: bare `cargo install --git <repo>` fails ("multiple packages with binaries"); name the target — `cargo install --git ... --locked pakmap` for the CLI. The GUI is **not** cargo-installable (see next trap).
- **Tauri dev-mode trap**: release GUI builds MUST pass `--features tauri/custom-protocol`, else `generate_context!` skips embedding `dist/` and the webview loads the vite devUrl ("Connection refused"). This is how `make gui-release` and `release.yml` build.
- **Boot smoke contract**: `xvfb-run -a dbus-run-session -- env PAKMAP_GUI_SMOKE_MS=9000 ./target/release/pakmap-gui` — exit **0** = webview booted and issued IPC, exit **3** = no IPC (the devUrl failure mode above). Both `ci.yml` (bare container = lib-less tray canary) and `release.yml` (installs `libayatana-appindicator` = tray-on canary) run it; don't remove either variant.
- `#![cfg_attr(test, allow(clippy::unwrap_used, ...))]` — `unwrap`/`expect` are `deny` in production code via root `[lints.clippy]`; integration test files re-allow at file top. No new unwraps in prod.
- **Version single source**: root `Cargo.toml` is it. `pakmap-landing/src/lib/version.ts` is auto-written by `scripts/sync-version.mjs` (predev/prebuild; commit the regenerated file). `tauri.conf.json` has no version field (inherits the crate). `gui/package.json`'s `version` is decorative and known-stale — leave it.
- **CSP ↔ inline script coupling**: `tauri.conf.json` script-src sha256-allowlists the exact bytes of the inline theme-bootstrap script in `gui/index.html`. Editing that script means recomputing the hash or the app fails CSP on first paint.
- **IPC layering**: frontend code must import backend calls only through `gui/src/lib/ipc.ts` (facade); `VITE_PAKMAP_MOCK_IPC=1` routes to the scripted `mockIpc.ts` gateway (dev/E2E only; tree-shaken from prod bundle). Don't call `@tauri-apps/*` APIs from components.
- The system tray must degrade, never crash: `libappindicator-sys` panics (not errors) when `libayatana-appindicator`.so is absent — `setup_tray` catches that unwind and logs. Keep tray-less boot green in lib-less environments.

## Packaging & release

- Tag push `v*` → `release.yml` (Arch container): verify gate → CLI tarball + GUI install-layout tarball (`usr/` tree) → GitHub release with auto-notes. Both tarballs must receive the boot-smoke QA (download the shipped GUI tarball, run the xvfb smoke, expect exit 0).
- `pakmap-landing/**` → `pages.yml` deploys the site to GitHub Pages.
- `sudo make install-gui` requires `make gui-release` run **as your user first** (sudo's `secure_path` drops `~/.local/bin`); install targets are copy-only with a fail-fast guard.
- Release URLs in `README.md` / `INSTALL.md` and landing version must be bumped in the same commit as the version bump.

## Conventions

- Feature passes: run the full gate battery first, then land as **one commit + one push** (owner's standing preference). Commits use `git -c user.name=achraf -c user.email=achraf@pacseek.local commit …`; the git identity `achraf <achraf@pacseek.local>` is expected in the log.
- GUI budget: shipped GUI binary ≤10M (`~9.5M` at v0.9 — measure after dependency changes). **fat `lto = true` is load-bearing for this**: measured locally (clang+native flags): fat = 9,967,144 B vs `lto = "thin"` = 12,074,952 B (+21%, over NFR). Do not change the LTO mode without a size re-check.
- gui styling: semantic Tailwind tokens (`--color-*` aliased to `--pk-*` primitives in `gui/src/index.css`); smallest text step `--text-nano`/`--text-2xs` (10–11px, no arbitrary sizes); icon buttons are 24×24 inside ≥40px touch targets.
