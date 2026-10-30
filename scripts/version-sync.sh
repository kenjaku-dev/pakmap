#!/usr/bin/env bash
# Version single-source check/fix. Four files claim a version; only Cargo.toml
# is real. The stale-PKGBUILD cost is well known in this repo's history, so
# the publish guard (scripts/aur-publish-guard.sh) runs this check first.
#
#   Cargo.toml [package] version        <- single source
#     install.sh                        PAKMAP_VERSION="..."
#     pakmap-landing/src/lib/version.ts export const PAKMAP_VERSION = "...";
#     packaging/aur/*/PKGBUILD          pkgver=...
#
#   scripts/version-sync.sh        check mode: exit 0 = every copy matches,
#                                  exit 1 = mismatch list on stderr
#   scripts/version-sync.sh --fix  rewrite the follower copies in place
#
# Release flow: bump Cargo.toml, run --fix, and commit ALL copies in the SAME
# commit (the repo's version-contract rule), then run the publish guard.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

die() { printf 'version-sync: %s\n' "$*" >&2; exit 1; }

cargo_version() {
  # [package] section of Cargo.toml (not [workspace.package], which holds no
  # version anyway; stop at the next section header).
  awk '
    /^\[package\]$/ { in_pkg = 1; next }
    /^\[/           { if (in_pkg) exit }
    in_pkg && /^version[ \t]*=/ {
      sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit
    }
  ' "$ROOT/Cargo.toml"
}

pin_of() { # install.sh PAKMAP_VERSION pin
  sed -n 's/^PAKMAP_VERSION="\(.*\)"$/\1/p' "$ROOT/install.sh" | head -n 1
}
landing_of() { # auto-generated landing version constant
  sed -n 's/^export const PAKMAP_VERSION = "\(.*\)";$/\1/p' \
    "$ROOT/pakmap-landing/src/lib/version.ts"
}
pkgbuild_of() { # pkgver of one PKGBUILD (unquoted template form)
  sed -n 's/^pkgver=\(.*\)$/\1/p' "$1" | head -n 1
}

FIX=0
case "${1:-}" in
  "") ;;
  --fix) FIX=1 ;;
  -h | --help) sed -n '2,16p' "$0"; exit 0 ;;
  *) die "unknown argument: $1 (use --fix, --help, or nothing)" ;;
esac

VER="$(cargo_version)"
[[ -n $VER ]] || die "could not read the [package] version from Cargo.toml"

fail=0

# install.sh pin
if [[ -f "$ROOT/install.sh" ]]; then
  if grep -q '^PAKMAP_VERSION="' "$ROOT/install.sh"; then
    current="$(pin_of)"
    if [[ $current != "$VER" ]]; then
      if [[ $FIX -eq 1 ]]; then
        sed -i "s/^PAKMAP_VERSION=\"[^\"]*\"/PAKMAP_VERSION=\"$VER\"/" "$ROOT/install.sh"
        echo "fixed install.sh PAKMAP_VERSION -> $VER"
      else
        printf \
          'MISMATCH install.sh PAKMAP_VERSION: %s (Cargo.toml: %s)\n' \
          "'$current'" "$VER" >&2
        fail=1
      fi
    fi
  else
    echo "install.sh: PAKMAP_VERSION pin line not found" >&2
    fail=1
  fi
fi

# landing version constant
if [[ -f "$ROOT/pakmap-landing/src/lib/version.ts" ]]; then
  if grep -q '^export const PAKMAP_VERSION' "$ROOT/pakmap-landing/src/lib/version.ts"; then
    current="$(landing_of)"
    if [[ $current != "$VER" ]]; then
      if [[ $FIX -eq 1 ]]; then
        sed -i \
          "s|^export const PAKMAP_VERSION = \".*\";|export const PAKMAP_VERSION = \"$VER\";|" \
          "$ROOT/pakmap-landing/src/lib/version.ts"
        echo "fixed pakmap-landing/src/lib/version.ts -> $VER"
      else
        printf \
          'MISMATCH pakmap-landing version.ts: %s (Cargo.toml: %s)\n' \
          "'$current'" "$VER" >&2
        fail=1
      fi
    fi
  else
    echo "pakmap-landing/src/lib/version.ts: PAKMAP_VERSION line not found" >&2
    fail=1
  fi
fi

# AUR PKGBUILD templates (one pkgver per package directory)
shopt -s nullglob
for kb in "$ROOT"/packaging/aur/*/PKGBUILD; do
  if grep -q '^pkgver=' "$kb"; then
    current="$(pkgbuild_of "$kb")"
    if [[ $current != "$VER" ]]; then
      if [[ $FIX -eq 1 ]]; then
        sed -i "s/^pkgver=.*/pkgver=$VER/" "$kb"
        echo "fixed $kb pkgver -> $VER"
      else
        printf 'MISMATCH %s pkgver: %s (Cargo.toml: %s)\n' \
          "${kb#"$ROOT"/}" "'$current'" "$VER" >&2
        fail=1
      fi
    fi
  else
    echo "$kb: pkgver line not found" >&2
    fail=1
  fi
done
shopt -u nullglob

if [[ $fail -eq 1 ]]; then
  exit 1
fi
if [[ $FIX -eq 1 ]]; then
  echo "all version copies now = $VER (commit in the SAME commit as the bump)"
fi
