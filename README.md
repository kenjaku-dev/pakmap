# pakmap

Fast **AUR + Official Repo + Flatpak** search **+ TUI install/remove/upgrade** for Arch / Artix — Rust, `ratatui` + `libalpm`.

One binary for both `pacman -Ss` and AUR `RPC v5`, with your drawing's terminal: `firefox` bar → results → `Enter` install / `i` info — plus `Tab` remover mode.

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

Built with **best skills**: `ratatui 0.30` (tui-design ecosystem-rust, immediate-mode, Layout cache) + `tui-design` (alternate screen, pane fallback, clutter audit, `NO_COLOR`) + `aur-guides` (RPC, makepkg, audit, never root).

## Features
- **One-shot CLI:** `pakmap <query>` repo first, aur after — parallel tokio, `libalpm` local DB, fallback `pacman -Ss`
- **TUI:** `pakmap --tui` or `pakmap` (no args, TTY) → tabs `Search` / `Installed` / `Updates` (`Tab` cycles) → search bar + virtualized `List` → `i` info+deps / `?` help popup → `Enter` confirm → `sudo pacman -S` (repo) or `git clone + makepkg -si` (AUR, PKGBUILD diff + preview, `namcap` if present, `~/.cache/pakmap`)
 - **Remover:** `Tab` → Installed mode (empty filter lists all, type to filter, `o` orphans-only `pacman -Qdt`) → `Enter`/`d`/`x` remove → confirm popup (`sudo pacman -Rs`, `pacman -Qi` preview) → `pakmap --remove <pkg>` for scripts (confirms on TTY; `PAKMAP_NOCONFIRM=1` bypasses)
 - **Updates:** `Tab` ×2 → Updates mode (repo `vercmp` + AUR RPC `/info` + flatpak `remote-info`, `[OOD]` badges) → `Enter` upgrade one, `U` upgrade all (`sudo pacman -Syu` + `flatpak update`, Arch-news guard) → `pakmap --updates` / `--upgrade` for scripts
 - **Flatpak:** `--source flatpak` search reads the **local appstream index directly** (`appstream.xml`, no `flatpak` spawn — ~150ms vs 3.4s) with bounded `flatpak search --json` as fail-open fallback; install/remove/update without sudo, `--remove` auto-detects app-IDs, `--clean` also drops unused runtimes; disable via `[behavior] flatpak_enabled=false`
 - **Orphans/stats:** `pakmap --orphans` (`-Qdt`), `pakmap --stats` (explicit/deps/orphans/updates/cache/flatpak) — all `--json` capable
 - **Refresh:** `F5`/`Ctrl+R` anywhere or `r` in list → confirm popup → `sudo pacman -Sy` → cache cleared + re-query → `pakmap --refresh` for scripts
 - Filters: `--source aur|repo|flatpak|all`, `--by name|name-desc|maintainer...`, `--limit N`, `--regex`, `--installed-only` (repo only), `--bottom-up`, `--json`, `--no-color`, `--no-tui`, `--remove PKG` (flatpak app-IDs auto-detected, no sudo), `--refresh`, `--updates` (repo+AUR+flatpak), `--orphans`, `--stats` (incl. flatpak apps/updates), `--upgrade` (pacman -Syu + flatpak update), `--clean [--keep N] [--dry-run]` (paccache + unused flatpak runtimes), `--readonly`
 - **Config:** `~/.config/pakmap/config.toml` (or `./pakmap.toml` project-local) — edit colors, borders, layout, timeouts without recompile — `pakmap --init-config` to generate, `--show-config` to locate, `--config PATH` to override
 - Safety: `aur-guides` — HTTPS sources, `makepkg` never as root via `nix::geteuid`, `PKGBUILD` diff + shown, `namcap`/`shellcheck` when present, Arch-news intervention guard before upgrade, `--readonly` browse mode, `tui-design` lifecycle `try_restore/try_init` + panic hook `color_eyre` before `ratatui::init`

## Install
```bash
# from the Arch release tarball (no toolchain needed):
curl -LO https://github.com/kenjaku-dev/pakmap/releases/download/v0.9.0/pakmap-0.9.0-x86_64-linux.tar.gz
tar xzf pakmap-0.9.0-x86_64-linux.tar.gz
sudo install -Dm755 pakmap /usr/local/bin/pakmap
# or straight from git (the workspace ships two binaries — name the CLI):
cargo install --git https://github.com/kenjaku-dev/pakmap --locked pakmap
# or build a local clone:
./install.sh  # automatic: deps + release build + install to /usr/local/bin
# ./install.sh --user  # no-sudo alternative (-> ~/.cargo/bin)
# manual:
cargo build --release  # 6.1M stripped, opt-level=z lto (reqwest 0.13 rustls → aws-lc-rs TLS provider)
sudo install -Dm755 target/release/pakmap /usr/local/bin/pakmap
sudo pacman -Sy  # refresh sync DB before first repo search
```

> **Note:** `cargo install --git https://github.com/kenjaku-dev/pakmap` *without* the trailing
> `pakmap` fails ("multiple packages with binaries found") — the workspace now also contains
> `pakmap-gui`. And the GUI is **not** cargo-installable: Tauri assets must be embedded by the
> documented builds above (AUR / release tarball / `make install-gui`); a bare cargo build of it
> is a dev-mode binary that points at the missing vite dev server.

Deps: `pacman 5.1+ (libalpm 16)`, `pacman-contrib` (`pacman-conf`), `git`, `base-devel` for AUR builds, `rust 1.90+` (MSRV 1.90 — workspace-wide pin).

### Desktop GUI (`pakmap-gui`, v0.9.0)

The Tauri companion app: same staged search across pacman/AUR/Flatpak, Updates tab (upgrade-all in one transaction), orphans + cache cleanup, Arch-news intervention banner — unprivileged; pacman/paccache flows go through `pkexec` (the polkit dialog is the confirmation, `--noconfirm` otherwise — no TTY in a GUI).

```bash
# from source:
make gui-release                            # vite dist + release binary (assets embedded)
sudo make install-gui                       # binary + .desktop + hicolor icons
# or AUR:
git clone https://aur.archlinux.org/pakmap-gui.git && (cd pakmap-gui && makepkg -si)
# or tarball (installs into /usr):
tar xzf pakmap-gui-0.9.0-x86_64-linux.tar.gz  # from GitHub releases, run as root
```

Extra deps: `webkit2gtk-4.1`, `nodejs` + `pnpm` (build time), `polkit`; `flatpak` optional. Details: `docs/gui-architecture.md`.

## Usage

**CLI (automation-friendly, no TUI):**
```bash
pakmap firefox --no-tui
pakmap firefox --limit 10 --source aur
pakmap neovim --by maintainer --limit 20
pakmap "linux.*headers" --regex
pakmap code --installed-only
pakmap rust --json | jq '.[].name'
pakmap firefox --bottom-up   # AUR first
pakmap --updates --json | jq '.[] | "\(.repo)/\(.name) \(.old_version) -> \(.new_version)"'
pakmap --orphans
pakmap --stats
pakmap --upgrade             # sudo pacman -Syu + flatpak update, news-guarded
pakmap --clean --dry-run     # preview reclaimable cache (paccache -k2) + flatpak note
pakmap --readonly --tui      # safe browse, mutating ops refused
```

**TUI (your drawing):**
```bash
pakmap --tui               # empty search, type firefox → Enter
pakmap --tui firefox       # prefilled query, immediate search
pakmap                     # no args + TTY => TUI (like original pakmap)
# Inside TUI: type query, Enter search, ↑↓/j k navigate, Enter install (confirm Y), i info+deps, U upgrade-all (Updates tab), o orphans-only (Installed), c clean cache (Installed/Updates), F5 or Ctrl+R refresh anywhere (r in list) (confirm Y), / search, Tab cycle Search→Installed→Updates (BackTab reverses), ? help, q quit
# Tab → Installed mode: type to filter, o orphans-only, Enter/d/x remove (confirm Y), F5 refresh anywhere (r in list) (confirm Y), Tab → Updates
# Tab ×2 → Updates mode: type to filter, Enter upgrade one (confirm Y), U upgrade all (confirm Y, news-guarded), Tab back to Search
# Install needs sudo password (flatpak needs none); AUR shows PKGBUILD diff + PKGBUILD + namcap/shellcheck then makepkg -si; remove runs sudo pacman -Rs (flatpak IDs uninstall without sudo); refresh runs sudo pacman -Sy; upgrade runs sudo pacman -Syu + flatpak update
# CLI shortcuts: pakmap --remove <pkg> | pakmap --refresh | pakmap --updates | pakmap --orphans | pakmap --stats | pakmap --upgrade | pakmap --clean --dry-run (confirm on TTY; PAKMAP_NOCONFIRM=1 for scripts; --readonly refuses mutating ops)
```

**Config (ez edit):**
```bash
pakmap --init-config          # creates ~/.config/pakmap/config.toml
pakmap --show-config          # prints paths (XDG + project-local if exists)
cat ~/.config/pakmap/config.toml
# edit: colors, borders, layout, timeouts — then restart pakmap
# [theme] border_focused="cyan" repo_aur="magenta bold" no_color=false tab_selected="black on cyan bold"
# [tui] floor_width=60 floor_height=14 show_tabs=true border="rounded" highlight_symbol="▸ " debounce_ms=400
# [search] limit=50 source="all" timeout_secs=15
# [behavior] makepkg_noconfirm=false remove_flags="Rs" remove_noconfirm=false refresh_noconfirm=false cache_dir="/tmp/pakmap" readonly=false news_enabled=true news_cache_secs=86400 upgrade_noconfirm=false clean_keep=2 clean_noconfirm=false flatpak_enabled=true flatpak_remote="flathub" flatpak_noconfirm=false
# Project-local override: ./pakmap.toml (same format) wins over XDG
pakmap --config ./my.toml firefox --no-tui
```

**Non-TTY / scripts:** `pakmap` auto-detects TTY; `echo | pakmap --tui` gives friendly `requires a terminal`. Use `--json` or `--no-tui` for pipes.

## Project Layout

```
src/
  main.rs        # cli + tui branch, IsTerminal, tokio, Config::load + CLI merge, --updates/--orphans/--stats/--upgrade/--readonly + news guard
  lib.rs         # re-exports
  cli.rs         # clap 4.5 derive, Option query, --tui/--no-tui, --config/--init-config/--show-config, --updates/--orphans/--stats/--upgrade/--readonly
  config.rs      # XDG ~/.config/pakmap/config.toml + ./pakmap.toml, TOML, theme/layout/behavior (serde) + readonly/news/diff/upgrade
  model.rs       # Package (PartialEq) + badges() [OOD]/[ORPHAN]/[UNMAINTAINED], depends/optdepends/required_by/optional_for/reason/orphan
  updates.rs     # repo vercmp + AUR /info batches (async+blocking), fail-open offline
  orphans.rs     # Qdt via reason==Depend + required/optional empty, fallback pacman -Qdt, package_details()
  news.rs        # Arch RSS cache-aside (news_cache_secs), intervention keywords, fail-open
  diff.rs        # PKGBUILD git diff + render_diff LCS + pager auto
  stats.rs       # explicit/deps/orphans/updates/cache + human_bytes
  search/
    aur.rs       # RPC v5 async + blocking (TUI), sorts popularity, config aur_rpc + timeout; shared tuned clients (gzip, 5s connect bound, nodelay) + one-shot per-process connection warm-up
    repo.rs      # alpm 5 + pacmanconf, fallback pacman -Ss
  output.rs      # colored + json (CLI) + print_updates/print_orphans/print_stats, badges + Required-by
  tui/
    mod.rs       # run() try_init/try_restore, run_with_config, suspend_and_run handoff + flush/drain
    app.rs       # Input (tui-input), ListState, Config stored, debounced/poll_ms from config, non-blocking mpsc with **staged per-backend results** (each backend paints on join, Done swaps atomically), Search/Installed/Updates + readonly + orphans_only + U upgrade
    ui.rs        # Layout vertical configurable, border_type + colors from theme, popups %, Clear hole-punch, 3 tabs + badges + deps + upgrade confirm
  install/
    repo.rs      # sudo pacman -S --needed (aur-helpers)
    aur.rs       # git clone/pull, PKGBUILD diff + bat/cat, namcap/shellcheck, makepkg -si, cache_dir + makepkg_noconfirm from config
    refresh.rs   # sudo pacman -Sy + refresh_noconfirm from config
    upgrade.rs   # sudo pacman -Syu + upgrade_noconfirm + readonly guard
    clean.rs     # paccache -d preview (parse files/bytes) + sudo paccache -r + readonly guard
    remove.rs    # sudo pacman -R + remove_flag_arg
  error.rs
  config.example.toml  # ship example
```

## Best Practices (skills)

- `ratatui` 0.30: `ratatui::try_init` before `color_eyre`, `Block::bordered().title`, `ListState` stateful, `TestBackend` snapshots at 80×24/60 cols, `unicode_width` via wrap, `Constraint::Min` responsive, `Clear` for popups
- `tui-design`: alternate screen, non-blocking disk/net (search via mpsc+thread parallel), `NO_COLOR` honor, `too-small` 60×13 fallback (3+8+1+1), clutter audit (<1 border, Spacing::Overlap), keyboard-reachable (`j/k`, `/`, `Enter`, `i`, `q`, `Ctrl+C`), suspend/resume with `clear`+`flush`+drain
- `aur-guides`: ` Aurweb RPC` `https://aur.archlinux.org/rpc/v5/search/{q}?by=`, `makepkg -s`, `namcap`, `.SRCINFO`, HTTPS sources
- Release `opt-level="z" lto codegen-units=1 strip`, `cargo fmt/clippy/audit`

## Performance

Measured `release` walls, steady state (see `context.md` for the full W-sprint record):

| leg | before | after |
|---|---|---|
| flatpak search | 3.4–4.5 s | **~150 ms** (23×) |
| AUR search | ~507 ms | **~320 ms** (gzip + tuned clients + warm pool) |
| all-sources | ~600 ms–3.1 s | **~490 ms** (now bounded by the repo leg) |

- Flatpak search reads `~/.local/share/flatpak/appstream/<remote>/<arch>/active/appstream.xml` (or the system install) directly and stops at the limit; `.gz`-only caches and oversized (>256MB) files fall back to the bounded `flatpak search` spawn.
- The TUI paints each backend's block the moment it finishes (repo first, then aur/flatpak) instead of waiting for the slowest one.
- Tests can pin the appstream lookup with `PAKMAP_FLATPAK_APPSTREAM_ROOT` (chaos fixtures); `PAKMAP_SKIP_LIVE=1` keeps all network warm-ups offline.

## Troubleshooting
- **No repo results:** `sudo pacman -Sy` (`/var/lib/pacman/sync/*.db`)
- **alpm init failed:** fallback `pacman -Ss`; check `pacman-conf` exists
- **AUR timeout:** retry or `--source repo`; TUI shows `No results`
- **Flatpak search empty (old flatpak / `.gz`-only cache):** falls back to bounded `flatpak search` automatically; `flatpak update` refreshes the index daily
- **TUI not starting:** `pakmap --tui` needs TTY; in headless use `--no-tui`; panic restores via `color_eyre` hook
- **AUR install as root:** blocked — run as user, `sudo` for pacman only

## Skills used
- `~/.config/opencode/skill/ratatui/SKILL.md` (mte90, 0.30.1)
- `~/.config/opencode/skill/tui-design/` + `references/ecosystem-rust.md`
- `~/.config/opencode/skill/aur-guides/` (`aur-rpc`, `aur-makepkg`)

## License
MIT

