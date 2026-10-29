#!/usr/bin/env bash
# pakmap — one script that installs AND updates, from prebuilt releases.
# Run it any time: fresh install when absent, in-place update when present.
#
# Usage:
#   ./install.sh               # CLI, + GUI automatically when it is installed
#   ./install.sh --user        # no sudo: ~/.local/bin + ~/.local/share
#   ./install.sh --gui         # force the desktop GUI install/update
#   ./install.sh --no-gui      # CLI only, skip the GUI even if installed
#   ./install.sh --latest      # newest GitHub release, not the pinned version below
#   ./install.sh --check       # print pinned vs installed vs latest; change nothing
#   ./install.sh --prefix /usr # custom system prefix (default /usr/local)
#   ./install.sh --no-sync     # skip the best-effort pacman sync-DB refresh
#   ./install.sh --yes         # non-confirm package/DB steps
# Source devs: rebuild-from-source updating lives in ./update.sh.
#
# VERSION CONTRACT: PAKMAP_VERSION below is bumped in the SAME commit as
# Cargo.toml / README / INSTALL / landing version on every release, so this
# script always matches the release it advertises (see AGENTS.md).
set -euo pipefail
PAKMAP_VERSION="0.9.2"
REPO_SLUG="kenjaku-dev/pakmap"

log() { printf '[install] %s\n' "$*"; }
warn() { printf '[install] WARNING: %s\n' "$*" >&2; }
die() { printf '[install] ERROR: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '2,15p' "$0"; }

# ---- flags -----------------------------------------------------------------
PREFIX="/usr/local"
USER_MODE=0; GUI_WANT=0; GUI_NO=0; LATEST=0; CHECK_ONLY=0; SYNC_DB=1; ASSUME_YES=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) PREFIX="${2:-}"; [[ -n $PREFIX ]] || die "--prefix needs a value"; shift 2 ;;
    --prefix=*) PREFIX="${1#*=}"; shift ;;
    --user) USER_MODE=1; shift ;;
    --gui) GUI_WANT=1; shift ;;
    --no-gui) GUI_NO=1; shift ;;
    --latest) LATEST=1; shift ;;
    --check) CHECK_ONLY=1; shift ;;
    --no-sync) SYNC_DB=0; shift ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *) die "unknown flag: $1 (see --help)" ;;
  esac
done
if [[ $GUI_WANT -eq 1 && $GUI_NO -eq 1 ]]; then
  die "--gui and --no-gui are mutually exclusive"
fi

command -v tar       >/dev/null 2>&1 || die "tar missing"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum missing (coreutils)"
FETCHER=""
if command -v curl >/dev/null 2>&1; then FETCHER="curl"
elif command -v wget >/dev/null 2>&1; then FETCHER="wget"
else die "curl or wget required to download the release"; fi
dl() { # dl <url> <out-file> — shellcheck disable=SC2317 (chosen per FETCHER)
  if [[ $FETCHER == curl ]]; then
    curl -fsSL --connect-timeout 20 "$1" -o "$2"
  else
    wget -q --timeout=20 "$1" -O "$2"
  fi
}

# pakmap needs libalpm: Arch-family only (same hard requirement as the CLI)
command -v pacman >/dev/null 2>&1 || \
  die "pacman not found — pakmap needs Arch/Artix/CachyOS/Endeavour (libalpm)"

# ---- versions: pinned vs installed vs latest --------------------------------
latest_release() {
  # HEAD on /releases/latest: the Location header carries the tag; no API quota.
  # Returns the BARE version ("0.9.2") — the leading 'v' from the tag is stripped.
  if [[ $FETCHER == wget ]]; then
    wget -q --max-redirect=0 --server-response --timeout=20 \
      "https://github.com/$REPO_SLUG/releases/latest" 2>&1 |
      grep -i '^[[:space:]]*[Ll]ocation:' | grep -o 'tag/.*' | cut -d/ -f2 |
      sed 's/^v//' || true
  else
    curl -fsIL -o /dev/null -w '%{url_effective}' \
      "https://github.com/$REPO_SLUG/releases/latest" | sed 's|.*/tag/||; s/^v//'
  fi
}
LATEST_TAG="$(latest_release)"
if [[ $LATEST -eq 1 ]]; then
  VERSION="${LATEST_TAG:-}"
  [[ -n $VERSION ]] || die "could not resolve the latest release tag"
  log "resolved latest release: v$VERSION"
else
  VERSION="$PAKMAP_VERSION"
fi

# ---- target paths ------------------------------------------------------------
if [[ $USER_MODE -eq 1 ]]; then
  PREFIX="$HOME/.local"; SHARE_ROOT="$HOME/.local/share"
else
  SHARE_ROOT="$PREFIX/share"
fi
CLI_DEST="$PREFIX/bin/pakmap"
GUI_DEST="$PREFIX/bin/pakmap-gui"
ICONS_ROOT="$SHARE_ROOT/icons/hicolor"

SUDO=""
if [[ $USER_MODE -eq 0 ]]; then
  mkdir -p "$PREFIX/bin" 2>/dev/null || true
  if [[ ! -d $PREFIX/bin || ! -w $PREFIX/bin ]]; then
    command -v sudo >/dev/null 2>&1 || die "$PREFIX/bin not writable and no sudo available"
    SUDO="sudo"
  fi
fi

GUI_HAVE=0
if [[ -x $GUI_DEST ]]; then
  GUI_HAVE=1
fi
if [[ $GUI_NO -eq 1 ]]; then
  GUI_DO=0
elif [[ $GUI_WANT -eq 1 ]]; then
  GUI_DO=1
else
  GUI_DO=$GUI_HAVE   # "install + update in one file": the GUI follows its presence
fi

installed_cli_version() {
  local out
  out="$("$CLI_DEST" --version 2>/dev/null || true)"
  printf '%s\n' "$out" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n 1 || true
}

if [[ $CHECK_ONLY -eq 1 ]]; then
  log "script pinned version : v$PAKMAP_VERSION$(if [[ $PAKMAP_VERSION != "$LATEST_TAG" ]]; then printf ' (latest is v%s — pull a fresh install.sh or use --latest)' "${LATEST_TAG:-?}"; fi)"
  log "current latest release : ${LATEST_TAG:+v}${LATEST_TAG:-unknown}"
  local_out="$(installed_cli_version || true)"
  log "installed CLI          : ${local_out:-none} ($CLI_DEST)"
  gui_state=no
  [[ $GUI_HAVE -eq 1 ]] && gui_state=yes
  log "installed GUI          : $gui_state ($GUI_DEST)"
  log "would install CLI      : v$VERSION -> $CLI_DEST"
  if [[ $GUI_DO -eq 1 ]]; then
    log "would install GUI      : v$VERSION -> $GUI_DEST (+ .desktop + icons, caches auto-refreshed)"
  else
    log "would install GUI      : skipped (--no-gui, or not installed yet — use --gui to force)"
  fi
  exit 0
fi

# ---- download ----------------------------------------------------------------
BASE_URL="https://github.com/$REPO_SLUG/releases/download/v$VERSION"
TMPDL="$(mktemp -d /tmp/opencode/pakmap-install.XXXXXX)"
PREV_CLI="$(installed_cli_version || true)"
trap 'rm -rf "$TMPDL"' EXIT
CLI_TARBALL="pakmap-$VERSION-x86_64-linux.tar.gz"
GUI_TARBALL="pakmap-gui-$VERSION-x86_64-linux.tar.gz"
if [[ $GUI_DO -eq 1 ]]; then
  log "downloading $CLI_TARBALL + $GUI_TARBALL ..."
  dl "$BASE_URL/$GUI_TARBALL" "$TMPDL/$GUI_TARBALL" || die "download failed: $GUI_TARBALL"
else
  log "downloading $CLI_TARBALL ..."
fi
dl "$BASE_URL/$CLI_TARBALL" "$TMPDL/$CLI_TARBALL" || die "download failed: $CLI_TARBALL"

# ---- checksums (SHASUMS256.txt ships from the release after v0.9.2 on) --------
if dl "$BASE_URL/SHASUMS256.txt" "$TMPDL/SHASUMS256.txt" 2>/dev/null && [[ -s $TMPDL/SHASUMS256.txt ]]; then
  grep -q "  $CLI_TARBALL" "$TMPDL/SHASUMS256.txt" || die "checksum manifest lacks $CLI_TARBALL"
  ( cd "$TMPDL" && sha256sum -c --ignore-missing SHASUMS256.txt ) || die "sha256 MISMATCH — download truncated or tampered"
  log "sha256 verified against SHASUMS256.txt"
else
  warn "no SHASUMS256.txt for v$VERSION (added after v0.9.2) — trusting GitHub HTTPS"
fi

# ---- pacman DB refresh (repo search results need /var/lib/pacman/sync) --------
if [[ $SYNC_DB -eq 1 ]]; then
  RUN_DB=1
  if [[ $EUID -ne 0 ]]; then
    command -v sudo >/dev/null 2>&1 || RUN_DB=0
  fi
  if [[ $RUN_DB -eq 1 ]]; then
    log "refreshing pacman sync DB (best-effort)"
    DB_FLAGS=()
    [[ $ASSUME_YES -eq 1 ]] && DB_FLAGS+=(--noconfirm)
    if [[ $EUID -eq 0 ]]; then
      pacman -Sy "${DB_FLAGS[@]}" >/dev/null 2>&1 || warn "DB refresh failed — continuing (repo search may be empty)"
    else
      sudo pacman -Sy "${DB_FLAGS[@]}" >/dev/null 2>&1 || warn "DB refresh failed — continuing (repo search may be empty)"
    fi
  fi
fi

install_file() { # install_file <mode> <src> <root> <dest-prefix>
  local mode="$1" src="$2" root="$3" dest_prefix="$4"
  local rel dest_path parent
  rel="${src#"$root"}"; rel="${rel#/}"
  dest_path="$dest_prefix/$rel"
  parent="$(dirname "$dest_path")"
  if [[ $SUDO == sudo ]]; then
    sudo mkdir -p "$parent"
    sudo install "-Dm$mode" "$src" "$dest_path"
  else
    mkdir -p "$parent"
    install "-Dm$mode" "$src" "$dest_path"
  fi
}

# ---- CLI install ------------------------------------------------------------
tar -xzf "$TMPDL/$CLI_TARBALL" -C "$TMPDL"
CLIBIN="$TMPDL/pakmap"
[[ -f $CLIBIN ]] || die "unexpected tarball layout: no pakmap at the root"
if [[ -n $PREV_CLI ]]; then
  log "CLI already installed (was v$PREV_CLI) — updating in place to v$VERSION"
else
  log "installing CLI v$VERSION -> $CLI_DEST"
fi
if [[ $SUDO == sudo ]]; then
  sudo install -Dm755 "$CLIBIN" "$CLI_DEST"
else
  install -Dm755 "$CLIBIN" "$CLI_DEST"
fi

# ---- GUI install -------------------------------------------------------------
if [[ $GUI_DO -eq 1 ]]; then
  mkdir -p "$TMPDL/gui"
  tar -xzf "$TMPDL/$GUI_TARBALL" -C "$TMPDL/gui"
  if [[ $GUI_HAVE -eq 1 ]]; then
    log "GUI already installed — updating in place to v$VERSION"
  else
    log "installing GUI v$VERSION -> $GUI_DEST (+ .desktop + hicolor icons)"
  fi
  while IFS= read -r -d '' f; do
    case "${f#"$TMPDL/gui/usr/"}" in
      bin/*) install_file 755 "$f" "$TMPDL/gui/usr" "$PREFIX" ;;
      *)     install_file 644 "$f" "$TMPDL/gui/usr" "$PREFIX" ;;
    esac
  done < <(find "$TMPDL/gui/usr" -type f -print0)
else
  log "GUI skipped (--no-gui, or not installed yet — use --gui to install it)"
fi

# ---- desktop icon refresh (runs on every install/update) ----------------------
if [[ $GUI_DO -eq 1 ]]; then
  if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    $SUDO gtk-update-icon-cache -qtf "$ICONS_ROOT" 2>/dev/null || \
      warn "icon cache refresh skipped for $ICONS_ROOT"
    log "hicolor icon cache refreshed"
  else
    warn "gtk-update-icon-cache not present — desktop shell still re-reads icons at next login"
  fi
  if command -v update-desktop-database >/dev/null 2>&1; then
    $SUDO update-desktop-database -q "$SHARE_ROOT/applications" 2>/dev/null || \
      warn "desktop database refresh skipped for $SHARE_ROOT/applications"
    log "application cache refreshed"
  fi
fi

# ---- runtime sanity -----------------------------------------------------------
RUNVER="$("$CLI_DEST" --version 2>/dev/null || true)"
log "CLI reports: ${RUNVER:-<no output>}"
if [[ $RUNVER != *" $VERSION"* ]]; then
  warn "installed CLI does not advertise v$VERSION (${RUNVER:-unknown})"
fi
if [[ $GUI_DO -eq 1 ]] && ! pacman -Q webkit2gtk-4.1 >/dev/null 2>&1; then
  warn "the GUI needs webkit2gtk-4.1 at runtime: sudo pacman -S webkit2gtk-4.1 (and polkit for pkexec prompts)"
fi
SMOKE="$("$CLI_DEST" firefox --no-tui --limit 2 --no-color 2>/dev/null || true)"
if [[ -n $SMOKE ]]; then
  log "smoke search OK"
else
  log "smoke search empty (offline, or the sync DB is empty) — binary still installed"
fi
# default config (best effort, never overwrites)
CONFIG_PATH="$("$CLI_DEST" --show-config 2>/dev/null | head -n 1 || true)"
if [[ -n $CONFIG_PATH && ! -f $CONFIG_PATH ]]; then
  "$CLI_DEST" --init-config >/dev/null 2>&1 || true
fi
log "done — CLI v$VERSION$([[ $GUI_DO -eq 1 ]] && printf ' + GUI') under $PREFIX"
log "re-run this file any time: same script = install and update"
