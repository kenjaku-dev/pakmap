# pakmap

**pakmap** searches **official pacman repos + AUR + Flatpak in one command** — and its full TUI installs, removes and upgrades packages without leaving the terminal.

One small Rust binary replaces `pacman -Ss` + AUR web searches + flatpak lookups: results arrive staged (repo first, then AUR), `Enter` installs — with a PKGBUILD diff preview for AUR builds — and a dedicated Updates tab handles upgrades with an Arch-news intervention guard.

Supported on **Arch and Artix**, MSRV **Rust 1.90**, `ratatui` TUI + `libalpm` FFI + `tokio` parallel searches.

[![CI](https://github.com/kenjaku-dev/pakmap/actions/workflows/ci.yml/badge.svg)](https://github.com/kenjaku-dev/pakmap/actions/workflows/ci.yml)
[![Release](https://github.com/kenjaku-dev/pakmap/actions/workflows/release.yml/badge.svg)](https://github.com/kenjaku-dev/pakmap/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.90%2B-orange)](#install)
<!-- TODO: verify — an "AUR version" badge needs real published packages; neither `pakmap` nor `pakmap-gui` existed on the AUR as of v0.10.2 (AUR RPC v5 returns 0 results), so the badge is withheld until they are published. -->

<!-- screenshot here: capture `pakmap --tui` mid-search (results list with repo/AUR/Flatpak rows) and link the image/GIF here -->

```
 🔍 Search (install) │ 🗑 Installed (remove) │ ⬆ Updates
┌ Search (Enter to search, Tab remover) ──────┐
│ firefox█                                    │
└─────────────────────────────────────────────┘
┌ Results (1/10) ─────────────────────────────┐
│▸ extra/firefox 123.0-1                       │
│    Fast browser                             │
│  aur/firefox-bin 123.0-1 (+1200 5.2)        │
│    Binary Firefox                           │
└─────────────────────────────────────────────┘
 Found 10 (repo 5 aur 4 flatpak 1)  Tab:cycle ?:help U:upgrade
```

## ✨ Features

- **One-shot CLI search** across all sources in parallel (`tokio`): repo via `libalpm`, AUR via RPC v5 — `pakmap <query>`
- **TUI** (`pakmap --tui` or `pakmap` with no args on a TTY): three tabs — `Search` / `Installed` / `Updates` (`Tab` cycles), virtualized results list, `i` for info + deps, `?` help popup
- **Install:** `Enter` installs — repo packages via `sudo pacman -S`, AUR via `git clone` + `makepkg -si` in `~/.cache/pakmap` with **PKGBUILD diff + preview** and `namcap`/`shellcheck` when present
- **Remove:** `Tab` → Installed mode, type to filter, `o` orphans-only — confirm popup runs `sudo pacman -Rs`; scripts use `pakmap --remove <pkg>`
- **Updates:** staged `vercmp` + AUR `/info` + flatpak `remote-info` with `[OOD]` badges — upgrade one (`Enter`) or all (`U`, Arch-news guard); scripts use `pakmap --updates` / `--upgrade`
- **Flatpak done right:** `--source flatpak` reads the **local appstream index directly** (~150 ms vs 3.4 s spawning `flatpak search`), bounded spawn as fail-open fallback; app-ID auto-detect on remove, `--clean` also drops unused runtimes
- **System chores:** `--orphans`, `--stats`, `--refresh`, `--upgrade`, `--clean [--keep N] [--dry-run]` — everything also `--json`-capable
- **Search filters:** `--source aur|repo|flatpak|all`, `--by name|name-desc|maintainer`, `--limit N`, `--regex`, `--installed-only`, `--bottom-up`, `--no-color`
- **Config:** `~/.config/pakmap/config.toml` (or project-local `./pakmap.toml`) — colors, borders, layout, timeouts, no recompile; 66-editable settings editable from CLI, TUI modal (`S`), and the GUI
- **Safety first:** AUR builds **never run as root** (`nix::geteuid`), PKGBUILD diff always shown, Arch-news intervention guard before upgrades, `--readonly` browse mode, `NO_COLOR` honored

## 📦 Install

### Installer script (Arch x86_64)

```bash
# one script: installs and updates; rerun it any time:
./install.sh              # pinned GitHub release, CLI + GUI-if-present (sha256-checked when the manifest ships)
# ./install.sh --user     # no sudo: -> ~/.local/bin (+ install --gui for the desktop app)
# ./install.sh --gui      # force the desktop GUI — .desktop + hicolor icons auto-refreshed
# ./install.sh --latest   # newest GitHub release instead of the pinned one
```

### From the AUR

<!-- TODO: verify — neither `pakmap` nor `pakmap-gui` was published on the AUR as of v0.10.2
     (checked live via RPC v5: 0 results). PKGBUILD templates are maintained in `packaging/aur/`;
     once published, restore:
     `git clone https://aur.archlinux.org/pakmap.git && (cd pakmap && makepkg -si)`
     `git clone https://aur.archlinux.org/pakmap-gui.git && (cd pakmap-gui && makepkg -si)` -->

### With cargo / from source

```bash
# install from git (the workspace ships two binaries — name the CLI):
cargo install --git https://github.com/kenjaku-dev/pakmap --locked pakmap

# or build manually:
cargo build --release     # 6.1M stripped, opt-level=z lto
sudo install -Dm755 target/release/pakmap /usr/local/bin/pakmap
sudo pacman -Sy           # refresh sync DB before first repo search
```

> **Note:** `cargo install --git https://github.com/kenjaku-dev/pakmap` *without* the trailing
> `pakmap` fails ("multiple packages with binaries found") — the workspace also contains
> `pakmap-gui`. And the GUI is **not** cargo-installable: Tauri assets must be embedded by the
> documented builds above; a bare cargo build of it is a dev-mode binary that points at a missing
> vite dev server.

**Dependencies:** `pacman 5.1+ (libalpm 16)`, `pacman-contrib` (`pacman-conf`), `git`, `base-devel` for AUR builds, `rust 1.90+` (MSRV — workspace-wide pin).

### Desktop GUI (`pakmap-gui`)

The Tauri companion app: same staged search across pacman/AUR/Flatpak, Updates tab (upgrade-all in one transaction), orphans + cache cleanup, Arch-news intervention banner — unprivileged; pacman/paccache flows go through `pkexec` (the polkit dialog is the confirmation, `--noconfirm` otherwise — no TTY in a GUI).

```bash
# from source:
make gui-release                            # vite dist + release binary (assets embedded)
sudo make install-gui                       # binary + .desktop + hicolor icons
# or the AUR (once the pakmap-gui package lands — see the note above):
# git clone https://aur.archlinux.org/pakmap-gui.git && (cd pakmap-gui && makepkg -si)
# or tarball (installs into /usr):
tar xzf pakmap-gui-0.10.2-x86_64-linux.tar.gz  # from GitHub releases, run as root
```

Extra deps: `webkit2gtk-4.1`, `nodejs` + `pnpm` (build time), `polkit`; `flatpak` optional. Details: [docs/gui-architecture.md](docs/gui-architecture.md).

## 🚀 Quick use

**CLI (automation-friendly, no TUI):**

```bash
pakmap firefox --no-tui            # results across repo + AUR
pakmap firefox --limit 10 --source aur
pakmap "linux.*headers" --regex
pakmap rust --json | jq '.[].name'
pakmap --updates                   # repo + AUR + flatpak updates
pakmap --orphans                   # install-as-dep, required-by-nothing
pakmap --stats                     # explicit/deps/orphans/updates/cache/flatpak
pakmap --upgrade                   # sudo pacman -Syu + flatpak update, news-guarded
pakmap --clean --dry-run           # preview reclaimable cache (paccache -k2)
pakmap --readonly --tui            # safe browse, mutating ops refused
```

**TUI:**

```bash
pakmap --tui           # empty search, type firefox → Enter
pakmap --tui firefox   # prefilled query, immediate search
pakmap                 # no args + TTY => TUI
```

Search is staged in the TUI as well — each backend paints its block the moment it finishes, so you never wait for the slowest source.

## ⌨️ TUI keybindings

The waterfall goes: a popup owns the keyboard first, then the `S` settings modal, then the frame.

| Key | Action | Where |
|---|---|---|
| `↑↓` / `j k` | move selection | list |
| `Enter` | search (search box) · install (Search) · remove (Installed) · upgrade one (Updates) | context |
| `i` | info + dependencies | list |
| `d` / `x` (`Del`/`BkSp` too) | remove package | Installed |
| `o` | orphans-only filter | Installed |
| `c` | clean cache (preview + confirm) | Installed, Updates |
| `U` | upgrade **all** (news-guarded) | Updates |
| `r` / `F5` / `Ctrl+R` | refresh sync DBs (confirm) | `r` in list, the rest anywhere |
| `F` | quick-filter visible results (type live, `Enter` arms sticky, `Esc`/`F` clears) | list |
| `T` | cycle sort: arrival → name → repo → version desc | list (idle only) |
| `y` | copy focused name (OSC 52) | list |
| `g` / `G` | jump to top / bottom of current view | list |
| `Tab` / `BackTab` | next / previous mode: Search → Installed → Updates | anywhere |
| `/` | jump to the search box | list |
| `Esc` | search ⇆ list | both |
| `S` | settings modal | list |
| `?` | help popup | list |
| `q` / `Ctrl+C` | quit | `q` in list, `Ctrl+C` anywhere |
| `y`/`Y`/`Enter` | confirm dialog | popups |
| `n`/`N`/`Esc`/`q` | dismiss | popups |

**Settings modal (`S`):** `←→` categories · `j/k` move · `/` filter · `Space` toggle booleans · `Enter` edit value · `r` reset · `s` save user file · `e` open file in `$EDITOR` · `S`/`Esc` close.

Flatpak installs need **no sudo**; repo/AUR refresh+install flows ask for a sudo password; AUR installs show the PKGBUILD diff first.

## ⚙️ Configuration

All configurable behavior lives in `~/.config/pakmap/config.toml` (falling back to project-local `./pakmap.toml`). **Three doors over one engine** — every key is editable from all three surfaces:

```bash
pakmap config list                       # every key: VALUE ORIGIN APPLY DANGER (--json --origin --section search)
pakmap config get search.timeout_secs    # effective value of one key (typo → "did you mean")
pakmap config set search.source aur      # writes the USER file: atomic, comments/spacing preserved
pakmap config set search.regex true --target project   # writes ./pakmap.toml (never created — trusted sections only)
pakmap config unset search.by            # drop an override
pakmap config validate                   # parse user + project files, report problems
pakmap config path                       # resolved config paths
pakmap --show-config                     # print paths and exit (pre-0.10.0 spelling)
pakmap config docs                       # generated example config (same bytes as --init-config)
pakmap --init-config                     # bootstrap a user config at the default path
pakmap --config ./my.toml firefox --no-tui  # --config replaces the user layer for this run
```

- **Precedence:** defaults < user file < project file — untrusted project layers never shadow unless `PAKMAP_TRUST_PROJECT=1`; **project files are never created by pakmap**
- **Writes are surgical:** comments, spacing and inline `# notes` preserved; a change on disk mid-edit triggers a fingerprint conflict gate
- **Every value validated** at parse time by the same engine (kind, enum membership, numeric clamps)
- **What applies when:** only `[theme]` keys live-apply in the TUI/GUI; everything else needs a relaunch; `no_color` is one-way once set
- The full reference table of all 66 settings (per key: kind, default, apply mode, danger class) is generated and drift-gated in [docs/settings.md](docs/settings.md)
- Deprecated alias: `[behavior] bottom_up` still reads as `[search] bottom_up` (the same-file canonical spelling wins); new writes must use the canonical key

**Environment variables:**

| Variable | Meaning |
|---|---|
| `PAKMAP_NOCONFIRM=1` | skip all confirmations (makepkg/remove/refresh/upgrade/clean/flatpak) |
| `PAKMAP_TRUST_PROJECT=1` | honor the project file wholesale, incl. `[behavior]` writes |
| `PAKMAP_SKIP_LIVE=1` | skip live pacman/AUR integration in tests |
| `PAKMAP_GUI_SMOKE_MS` | GUI smoke-boot budget in ms (CI canary) |
| `PAKMAP_GUI_FPBUDGET_MS` | GUI first-IPC latency budget in ms |

**Non-TTY / scripts:** `pakmap` auto-detects TTY; `echo | pakmap --tui` gives a friendly hint `requires a terminal`. Use `--json` or `--no-tui` for pipes.

## 📊 Performance

Measured `release` walls, steady state (full W-sprint record in `context.md`):

| leg | before | after |
|---|---|---|
| flatpak search | 3.4–4.5 s | **~150 ms** (23×) |
| AUR search | ~507 ms | **~320 ms** (gzip + tuned clients + warm pool) |
| all-sources | ~600 ms–3.1 s | **~490 ms** (now bounded by the repo leg) |

- Flatpak search reads `~/.local/share/flatpak/appstream/<remote>/<arch>/active/appstream.xml` directly and stops at the limit; `.gz`-only caches and overlong (>256 MB) files fall back to the bounded `flatpak search` spawn.
- The TUI paints each backend's block the moment it finishes instead of waiting for the slowest one.
- Tests can pin the appstream lookup with `PAKMAP_FLATPAK_APPSTREAM_ROOT` (chaos fixtures); `PAKMAP_SKIP_LIVE=1` keeps all network warm-ups offline.

## 🩺 Troubleshooting

- **No repo results:** `sudo pacman -Sy` (`/var/lib/pacman/sync/*.db`)
- **`alpm init failed`:** fallback `pacman -Ss` kicks in; check `pacman-conf` exists
- **AUR timeout:** retry or `--source repo`; TUI shows `No results`
- **Flatpak search empty (old flatpak / `.gz`-only cache):** bounded `flatpak search` fallback is automatic; `flatpak update` refreshes the index daily
- **TUI not starting:** `pakmap --tui` needs a TTY; in headless use `--no-tui`; panic restores via `color_eyre` hook
- **`y` yank does nothing:** OSC 52 needs a cooperating terminal (gnome-terminal panels eat it); `copied:` flash always shows even when the panel swallows the sequence
- **AUR install as root:** deliberately blocked — run as user, `sudo` only for pacman flows

## 🗂 Project Layout

```
src/
  main.rs        # cli + tui branch, IsTerminal, tokio, Config load + CLI merge, --updates/--orphans/--stats/--upgrade/--readonly + news guard
  lib.rs         # re-exports
  cli.rs         # clap 4.5 derive, Option query, --tui/--no-tui, --config/--init-config/--show-config
  config.rs      # XDG ~/.config/pakmap/config.toml + ./pakmap.toml, theme/layout/behavior (serde) + readonly/news/diff/upgrade
  model.rs       # Package + badges() [OOD]/[ORPHAN]/[UNMAINTAINED], depends/optdepends/required_by/orphan
  updates.rs     # repo vercmp + AUR /info batches (async+blocking), fail-open offline
  orphans.rs     # Qdt via reason==Depend + required/optional empty, fallback pacman -Qdt
  news.rs        # Arch RSS cache-aside, intervention keywords, fail-open
  diff.rs        # PKGBUILD git diff + render_diff LCS + pager auto
  stats.rs       # explicit/deps/orphans/updates/cache + human_bytes
  search/        # aur.rs (RPC v5 async+blocking, tuned clients) + repo.rs (alpm 5 + pacmanconf, fallback pacman -Ss)
  output.rs      # colored + json (CLI), print_updates/print_orphans/print_stats
  tui/           # mod.rs run loop · app.rs Input/Mode machine · ui.rs layout+popups; keys.rs — **single-source keymap** (footer+help generated)
  install/       # repo.rs (sudo pacman -S) · aur.rs (PKGBUILD diff + makepkg -si) · refresh.rs · upgrade.rs · clean.rs · remove.rs
  settings/      # one catalog (keys/kinds/danger/apply), layered resolver, comment-preserving atomic writer — powers CLI `config`, TUI modal, GUI screen
error.rs · config.example.toml
```

## 🛠 Development notes

- **TUI stack:** `ratatui 0.30` — `try_init` before `color_eyre`, stateful `ListState`, `TestBackend` snapshots, `unicode_width`, `Constraint::Min` responsive layout, `Clear` for popups
- **Design discipline:** alternate screen, non-blocking disk/net (`mpsc` search thread), `NO_COLOR` honored, 60×13 too-small fallback, clutter audit (<1 border, `Spacing::Overlap`), keyboard-first (`j/k`, `Esc`, `?`)
- **AUR handling:** RPC v5 endpoints, `makepkg` discipline, `namcap`, `.SRCINFO`, HTTPS sources only
- **Release profile:** `opt-level="z"`, fat `lto`, `codegen-units=1`, `strip`; `cargo fmt/clippy/audit` as the gate battery — see [CONTRIBUTING.md](CONTRIBUTING.md) to build and test locally

## 🤝 Contributing

Bug reports, feature ideas, docs, code — all welcome. Setup, workflow, and code standards live in [CONTRIBUTING.md](CONTRIBUTING.md).

## 📄 License

**MIT** — see [LICENSE](LICENSE).
