#!/usr/bin/env bash
# pakmap manual updater: fast-forward pull + release rebuild + reinstall.
# Usage:
#   ./update.sh                 # CLI: rebuild + sudo reinstall to /usr/local/bin
#   ./update.sh --user          # CLI: rebuild + reinstall to ~/.cargo/bin (no sudo)
#   ./update.sh --prefix /usr   # system reinstall to a custom prefix
#   ./update.sh --gui           # also rebuild + reinstall pakmap-gui (assets + .desktop + icons)
#   ./update.sh --no-gui        # CLI only, even if pakmap-gui is already installed
#   ./update.sh --check         # show repo vs installed versions, change nothing
#
# GUI inclusion: automatic when pakmap-gui is already installed at the target
# prefix (or ~/.local/bin for --user); otherwise only with an explicit --gui,
# because the GUI needs pnpm + webkit2gtk and is a much heavier build.
# Safety:
#   - refuses on a dirty working tree (commit or stash first)
#   - git pull is --ff-only: never merges or rewrites your local commits
#   - unpushed local commits are kept as-is when the remote is an ancestor
#   - the GUI build runs as YOUR user (pnpm needs ~/.local/bin on PATH); only
#     the copy-time install step asks for sudo
set -euo pipefail

PREFIX="/usr/local"
USER_MODE=0
CHECK_ONLY=0
GUI_WANT=0
GUI_NO=0
BIN_NAME="pakmap"
GUI_BIN_NAME="pakmap-gui"

log() { printf '[update] %s\n' "$*"; }
die() { printf '[update] ERROR: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,17p' "$0"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      PREFIX="${2:-}"
      if [[ -z "$PREFIX" ]]; then die "--prefix needs a value"; fi
      shift 2
      ;;
    --prefix=*) PREFIX="${1#*=}"; shift ;;
    --user) USER_MODE=1; shift ;;
    --gui) GUI_WANT=1; shift ;;
    --no-gui) GUI_NO=1; shift ;;
    --check) CHECK_ONLY=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *) die "unknown flag: $1 (see --help)" ;;
  esac
done
if [[ $GUI_WANT -eq 1 && $GUI_NO -eq 1 ]]; then
  die "--gui and --no-gui are mutually exclusive"
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

# Fail fast: a broken toolchain (Arch Rust/LLVM ABI skew) otherwise dies
# mid-build with a cryptic `rustc -vV` error.
if ! rustc -vV >/dev/null 2>&1; then
  die "rustc is broken ($(rustc -vV 2>&1 | head -n 1)). Run: sudo pacman -Syu (partial upgrades break the Rust/LLVM ABI), then re-run $0"
fi

repo_version() {
  grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2
}

# GUI target dir mirrors the CLI one ({PREFIX}/bin, or ~/.local/bin in
# --user mode — matching where AGENTS.md points user-level installs).
gui_target_path() {
  if [[ $USER_MODE -eq 1 ]]; then
    local base="${PAKMAP_USER_BIN:-$HOME/.local/bin}"
    printf '%s/%s\n' "$base" "$GUI_BIN_NAME"
  else
    printf '%s/bin/%s\n' "$PREFIX" "$GUI_BIN_NAME"
  fi
}

GUI_UPDATE=0
gui_target="$(gui_target_path)"
if [[ $GUI_NO -eq 0 && ( $GUI_WANT -eq 1 || -x "$gui_target" ) ]]; then
  GUI_UPDATE=1
fi

installed_version() {
  # $1 = binary path; prints version or "not installed"
  if [[ -x "$1" ]]; then
    "$1" --version 2>/dev/null || echo "installed (no --version)"
  else
    echo "not installed"
  fi
}

if [[ $CHECK_ONLY -eq 1 ]]; then
  echo "repo:         $(repo_version) ($(git log --oneline -1))"
  echo "/usr/local:   $(installed_version /usr/local/bin/$BIN_NAME)"
  echo "\$HOME/.cargo: $(installed_version "$HOME/.cargo/bin/$BIN_NAME")"
  echo "in PATH:      $(installed_version "$(command -v $BIN_NAME 2>/dev/null || echo /nonexistent)")"
  echo "GUI target:   $gui_target ($(if [[ -x "$gui_target" ]]; then echo installed; else echo "not installed"; fi)) — updater includes it: $([[ $GUI_UPDATE -eq 1 ]] && echo yes || echo no)"
  git fetch origin -q 2>/dev/null || true
  git status --short --branch | head -n 5
  exit 0
fi

# 1. Refuse on dirty tree — updating over uncommitted work is how code gets lost.
if [[ -n "$(git status --porcelain)" ]]; then
  git status --short | head -n 10
  die "working tree dirty — commit or stash first, then re-run"
fi

# 2. Fast-forward only: keeps unpushed local commits, never merges.
log "pulling latest (ff-only)..."
git pull --ff-only 2>&1 | tail -n 3 || die "git pull --ff-only failed (diverged history?) — resolve manually, then re-run"

log "repo version: $(repo_version)"

# 3. Rebuild CLI (always).
log "building CLI (release)..."
cargo build --release --locked
ls -lh "target/release/$BIN_NAME"

# 3b. Rebuild GUI (only when included). The pnpm build must run as this
# user — PATH needs pnpm (often ~/.local/bin, per the repo's env quirk).
if [[ $GUI_UPDATE -eq 1 ]]; then
  if ! command -v pnpm >/dev/null 2>&1; then
    if [[ -x "$HOME/.local/bin/pnpm" ]]; then
      export PATH="$HOME/.local/bin:$PATH"
    else
      die "pnpm not found (needed for the GUI frontend) — pacman -S pnpm or cargo-install it, then re-run (or pass --no-gui)"
    fi
  fi
  if ! command -v webkit2gtk-4.1 >/dev/null 2>&1 \
     && ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    die "webkit2gtk-4.1 not found (GUI dependency) — sudo pacman -S webkit2gtk-4.1, then re-run (or pass --no-gui)"
  fi
  log "building GUI (release, embedded assets)..."
  make gui-release
  ls -lh "target/release/$GUI_BIN_NAME"
fi

# 4. Reinstall to the same place the user already uses.
if [[ $USER_MODE -eq 1 ]]; then
  log "installing CLI to ~/.cargo/bin via cargo install"
  cargo install --path . --force --locked
  TARGET="$HOME/.cargo/bin/$BIN_NAME"
  [[ ":$PATH:" == *":$HOME/.cargo/bin:"* ]] || log "NOTE: ~/.cargo/bin not in PATH — your shell uses $(command -v $BIN_NAME)"
  if [[ $GUI_UPDATE -eq 1 ]]; then
    GUI_DEST="$gui_target"
    mkdir -p "$(dirname "$GUI_DEST")"
    log "installing GUI to $GUI_DEST (user-mode: assets ride along per make install-gui layout)"
    # user-mode GUI: same copy-only layout as `make install-gui`, $HOME-owned
    install -Dm755 "target/release/$GUI_BIN_NAME" "$GUI_DEST"
    GUI_PREFIX_BASE="$(dirname "$GUI_DEST")/.."   # bin/.. so share/ lands beside bin/
    make install-gui-assets "DESTDIR=" "PREFIX=$GUI_PREFIX_BASE"
    GUI_TARGET="$GUI_DEST"
  fi
else
  SUDO=""
  if [[ $EUID -ne 0 ]]; then
    command -v sudo >/dev/null 2>&1 || die "need root or sudo to write $PREFIX/bin"
    SUDO="sudo"
  fi
  DEST="$PREFIX/bin/$BIN_NAME"
  log "installing CLI to $DEST"
  $SUDO install -Dm755 "target/release/$BIN_NAME" "$DEST"
  TARGET="$DEST"
  if [[ $GUI_UPDATE -eq 1 ]]; then
    log "installing GUI to $PREFIX/bin (assets + .desktop + hicolor icons)"
    # copy-only step; gui-release already ran as this user (the Makefile's
    # sudo PATH trap is avoided by construction)
    $SUDO make install-gui "DESTDIR=" "PREFIX=$PREFIX"
    GUI_TARGET="$PREFIX/bin/$GUI_BIN_NAME"
  fi
fi

# 5. Verify (captured, not piped: SIGPIPE + pipefail would fake a failure).
log "verifying..."
"$TARGET" --version
INSTALLED_VER="$("$TARGET" --version 2>/dev/null || true)"
if [[ "$INSTALLED_VER" != *"$(repo_version)"* ]]; then
  log "WARNING: installed ($INSTALLED_VER) differs from repo $(repo_version)"
fi
if [[ $GUI_UPDATE -eq 1 ]]; then
  if [[ -x "$GUI_TARGET" ]]; then
    ls -lh "$GUI_TARGET"
    log "GUI installed (boot it from the app menu: pakmap-gui)"
  else
    log "WARNING: GUI target $GUI_TARGET missing after install — check the make output above"
  fi
fi
SMOKE_OUT="$("$TARGET" firefox --no-tui --limit 2 --no-color 2>&1 || true)"
if [[ -n "$SMOKE_OUT" ]]; then
  printf '%s\n' "$SMOKE_OUT" | head -n 8
  log "smoke search OK"
else
  log "smoke search had no output (AUR offline or empty DB?) — binary still installed"
fi

log "done: $TARGET ($("$TARGET" --version))"
