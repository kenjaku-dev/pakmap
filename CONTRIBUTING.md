# Contributing to pakmap

Thanks for taking an interest in pakmap — a fast, single-binary package search + TUI for Arch/Artix. Every kind of contribution is genuinely welcome, from a one-line typo fix to a new search backend.

The instructions here are deliberately short: **if in doubt, open an issue and ask first.** Small, focused PRs merge fastest.

## Ways to contribute

- **Report bugs** — wrong results, crashes, TUI glitches, broken flags → see *Reporting bugs* below
- **Suggest features** — open an issue with what you'd like and why; realistic scope beats wishlist size
- **Improve code** — performance, refactors, error handling, new search filters
- **Documentation** — README, `docs/`, `--help` text, this file
- **Triage & tests** — reproduce reported issues, add missing unit tests

## Reporting bugs

A good bug report contains:

1. **Version:** output of `pakmap --version` (and `pakmap-gui` version if applicable)
2. **Environment:** distro (Arch/Artix), terminal (and how the TUI/GUI is being run)
3. **Steps to reproduce:** minimal, copyable command(s)
4. **Output:** paste the actual text (use `-v` for verbose tracing; `--json` where relevant)
5. **Expected vs actual** behavior

Security-sensitive issues (anything involving `--readonly`, sudo/root boundaries, AUR install safety) — please don't open a public issue; use GitHub's private security advisory or contact the maintainer directly.

## Development setup

pakmap links **libalpm** directly, so development realistically requires an **Arch or Artix system** with a current pacman (≥ 5.1 / libalpm 16, matching what CI builds against).

**Requirements:**

- **Rust stable** — the workspace pins `rust-version = 1.90` (MSRV); avoid APIs newer than that
- **pacman 5.1+** (the `libalpm` dev bindings ship with pacman itself on Arch)
- **pacman-contrib** (`pacman-conf`), **git**, and **base-devel** (AUR builds)
- For the **GUI** (`gui/src-tauri`): `webkit2gtk-4.1`, `nodejs` + `pnpm` (build time), `polkit`; `flatpak` optional

**Clone, build, run:**

```bash
git clone https://github.com/kenjaku-dev/pakmap.git
cd pakmap
cargo build --release
cargo run -- --tui          # the TUI needs a real TTY
cargo run -- firefox --no-tui
```

**Tests** (offline first — the exact way CI's rust job runs them):

```bash
PAKMAP_SKIP_LIVE=1 cargo test --locked --lib   # fully offline unit tests
```

The live integration tests (`tests/integration_search`, `make test`) touch real pacman/AUR state — run `sudo pacman -Sy` first, then `cargo test --workspace --locked`.

**The full gate battery** (CI mirrors it — run before you push):

```bash
cargo fmt --all --check                     # 1. formatting first
cargo clippy --workspace --all-targets --locked -- -D warnings   # 2. then lints
cargo test --workspace --locked             # 3. then tests
(cd gui && pnpm typecheck && pnpm lint && pnpm test && pnpm build)   # GUI crate
(cd gui && pnpm exec playwright test)       # e2e — always last
```

## Workflow

**Fork → Branch → Commit → Pull Request:**

1. **Fork** the repo and clone your fork
2. **Branch** off `main`, named by intent: `fix/text-not-resizing`, `feat/flatpak-search`, `docs/keybindings-table`
3. **Commit** using [Conventional Commits](https://www.conventionalcommits.org/) — this repo's log already uses them: `feat(tui): …`, `fix(install): …`, `docs: …`, `perf: …`, `refactor: …`, `test: …`, `chore: …` (scope optional). One logical change per PR is ideal
4. **PR** against `main`: describe *what* changed, *why*, and *how you verified it* (test names/commands). Link the issue it fixes (`fixes #123`). Keep the diff focused — drive-by refactors belong in their own PR

CI runs the full gate battery above; green CI + a focused diff is what we look for.

## Code standards

- **Style:** ship `cargo fmt`-clean; `cargo clippy` **must** be warning-free (`-D warnings`)
- **No panics in production code:** the workspace denies `unwrap`/`expect` outside tests (`[lints.clippy]` in the root `Cargo.toml`) — prefer `anyhow`/`thiserror` propagation or graceful fallback
- **Test what you fix:** a bug regression gets a failing test first; new features ship with tests. The TUI has byte-exact snapshot suites — update outputs deliberately, not by pasting your terminal
- **Don't weaken the safety model.** This is non-negotiable:
  - `--readonly` and `[behavior] readonly` must refuse every mutating operation
  - pacman flows are the **only** things allowed `sudo`; AUR builds never run as root
  - the PKGBUILD diff is always shown before an AUR install; confirmation gates stay on unless `PAKMAP_NOCONFIRM=1` was explicitly set
  - new network access (endpoints, timeouts, blocking calls) follows the existing fail-open patterns and gets discussed first
- **Single-source tables are sacred:** the TUI keymap (`src/tui/keys.rs`) and the settings catalog (`src/settings/catalog.rs`) *generate* their docs (footer/help legend, `docs/settings.md`) behind drift-gate tests. If your change edits a binding or key, the generated copy must change too — the test will remind you by failing

### Repo quirks worth knowing

- `docs/settings.md` is generated: regenerate with `PAKMAP_REGEN_DOCS=1 cargo test --lib settings::command` and commit the result with any catalog change
- The GUI release build must embed assets: build it with `make gui-release` (which passes `--features tauri/custom-protocol`) — a bare `cargo build -p pakmap-gui` produces a dev-mode binary
- The shipped GUI binary has a hard size budget (~10.5 MB) enforced by release QA; fat LTO + `codegen-units=1` in `[profile.release]` is load-bearing for it — don't flip profile settings without re-measuring
- The workspace contains **two** binaries (`pakmap`, `pakmap-gui`); workspace-level cargo commands operate on the CLI crate via `default-members`, GUI commands use explicit `-p pakmap-gui`

## Code of conduct

Keep it simple and respectful: be kind to beginners, critique code not people, assume good faith, and keep discussions on-topic and constructive. Hostile behavior may result in removal from the project. (A full Contributor Covenant copy may land here later — the rules above are the deal for now.)

## Good first issues

We add a `good first issue` label to small, well-scoped issues. Great entry points if you're new:

- **Better error messages** — the CLI surfaces a handful of terse errors (`alpm init failed`, AUR timeouts); turning them into *what happened / why / what to do now* text is high-value and small-scope
- **Unit tests** — several modules (`stats.rs` human formatting, diff rendering, output badge logic, config parsing edge cases) accept isolated tests without touching libalpm or the network
- **`--help` documentation** — flag descriptions in `src/cli.rs` are clap doc-comments; tightening/clarifying them (and matching README examples) is a perfect first PR

Look for issues labeled [`good first issue`](https://github.com/kenjaku-dev/pakmap/labels/good%20first%20issue) — and if you're not sure where to start, open a fresh issue titled "I'd like to contribute, where?"

## License

By contributing, you agree that your contributions are licensed under the project's [MIT License](LICENSE).
