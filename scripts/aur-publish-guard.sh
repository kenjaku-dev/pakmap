#!/usr/bin/env bash
# Fail-closed pre-flight for publishing the AUR packages. The AUR update
# itself stays a manual step (per packaging/aur/*/README.md); this script is
# the checklist that refuses unless the local repo is in the exact
# publishable state for a given tag.
#
#   scripts/aur-publish-guard.sh vX.Y.Z [--network]
#
# Checks (any failure -> exit 1, publish nothing):
#   1. HEAD is EXACTLY tagged vX.Y.Z — no untagged commits slipped in
#      after the release tag.
#   2. No uncommitted tracked changes (untracked scratch is ignored).
#   3. HEAD is reachable from at least one remote-tracking branch —
#      publishing a tarball URL for a ref no remote knows is a footgun.
#   4. scripts/version-sync.sh passes — no version drift in the tree.
#   5. Every packaging/aur/*/PKGBUILD pins the tag in its source URL
#      (template form refs/tags/v${pkgver} with pkgver == VER, or a
#      hardcoded refs/tags/vX.Y.Z).
#   --network: ALSO fetches the tagged source tarball from GitHub — the
#      exact URL the PKGBUILD derives — and prints its sha256 so the
#      real sum can replace sha256sums=('SKIP') at publish time.
#      Missing/unreachable tarball -> guard fails (tag pushed but assets
#      not built yet is a "wait", not a "publish now").
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

die() { printf 'aur-guard: %s\n' "$*" >&2; exit 1; }

TAG=""
NETWORK=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --network) NETWORK=1; shift ;;
    v*) TAG="$1"; shift ;;
   *) die "usage: scripts/aur-publish-guard.sh vX.Y.Z [--network]" ;;
  esac
done
[[ -n $TAG ]] || die "usage: scripts/aur-publish-guard.sh vX.Y.Z [--network]"
[[ $TAG =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "tag must look like vX.Y.Z, got '$TAG'"
VER="${TAG#v}"

command -v git >/dev/null 2>&1 || die "git missing"

# 1 — the release commit is exactly HEAD
if ! git describe --exact-match --tags --match "$TAG" HEAD >/dev/null 2>&1; then
  die "HEAD is not tagged $TAG (exact match required) — tag the release commit first"
fi

# 2 — clean tracked tree
if ! git diff --quiet; then
  die "uncommitted tracked changes (unstaged)"
fi
if ! git diff --cached --quiet; then
  die "uncommitted tracked changes (staged)"
fi

# 3 — pushed somewhere
if ! git branch -r --contains HEAD 2>/dev/null | grep -q .; then
  die "HEAD is not reachable from any remote branch — push commits AND tag first"
fi

# 4 — single-source version state (also hard-caps pkgver == VER)
if ! bash "$ROOT/scripts/version-sync.sh"; then
  die "version drift — run scripts/version-sync.sh --fix in the SAME commit as the bump"
fi

# 5 — PKGBUILD source URLs pin the tag
shopt -s nullglob
pkgs=("$ROOT"/packaging/aur/*/PKGBUILD)
shopt -u nullglob
if [[ ${#pkgs[@]} -eq 0 ]]; then
  die "no PKGBUILD found under packaging/aur/"
fi
for kb in "${pkgs[@]}"; do
  if ! grep -Fq 'refs/tags/v${pkgver}' "$kb" && ! grep -Fq "refs/tags/v$VER" "$kb"; then
    die "$(basename "$(dirname "$kb")"): PKGBUILD does not pin $TAG in its source URL"
  fi
done

# 6 — optional live check of the tarball the PKGBUILD will resolve to
if [[ $NETWORK -eq 1 ]]; then
  command -v curl >/dev/null 2>&1 || die "--network needs curl"
  REPO_URL="$(awk '/^repository *= */ { sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit }' \
    Cargo.toml)"
  [[ -n $REPO_URL ]] || die "could not read the repository URL from Cargo.toml"
  URL="$REPO_URL/archive/refs/tags/$TAG.tar.gz"
  TMPF="$(mktemp "${TMPDIR:-/tmp}/pakmap-guard-src.XXXXXX")"
  trap 'rm -f "$TMPF"' EXIT
  if ! curl -fsSL --proto '=https' "$URL" -o "$TMPF"; then
    die "source tarball not reachable yet ($URL) — tag pushed but source still building?"
  fi
  echo "source tarball: $URL"
  sha256sum "$TMPF" |
    awk '{print "  sha256: " $1 "   <- replace the PKGBUILD sha256sums=(SKIP) with this"}'
fi

echo "aur-guard: OK — repo state is publishable for $TAG"
