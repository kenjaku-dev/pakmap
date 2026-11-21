#!/usr/bin/env bash
# Release gate: the CLI tarball must carry every integration artifact that
# `install.sh` and the AUR PKGBUILD know how to install. Generated from the
# binary's own clap parser in release.yml — never hand-written.
set -euo pipefail

tarball="${1:-}"
if [[ -z $tarball || ! -f $tarball ]]; then
  printf 'usage: %s <pakmap-VERSION-x86_64-linux.tar.gz>\n' "$0" >&2
  exit 2
fi

entries="$(tar -tzf "$tarball")"
missing=0
for name in pakmap pakmap.bash _pakmap pakmap.fish pakmap.1; do
  if ! printf '%s\n' "$entries" | grep -qx "\./$name"; then
    printf 'release tarball is missing ./%s\n' "$name" >&2
    missing=1
  fi
done
if [[ $missing -ne 0 ]]; then
  printf 'incomplete release artifact: %s\n' "$tarball" >&2
  exit 1
fi
printf 'release asset check OK: %s\n' "$tarball"
