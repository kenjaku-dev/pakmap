# pakmap — AUR publishing

`PKGBUILD` here is the source of truth for the CLI package. Publishing steps,
per release:

1. `pkgver=…` + `pkgrel=1`
2. Real checksum: `updpkgsums` (or `makepkg -g >> PKGBUILD`) once the tag exists
3. `namcap PKGBUILD` if available
4. `makepkg --printsrcinfo > .SRCINFO`
5. Copy `PKGBUILD` + `.SRCINFO` to the `pakmap` AUR git repo, commit, push

Notes:
- build runs `cargo build --release --locked` against the GitHub tag tarball,
  then the binary generates its own completions + man page
  (`--generate-completions bash|zsh|fish`, `--generate-man`) — they can never
  drift from the clap parser
- deps: `pacman>=5.1 pacman-contrib git base-devel`; `makedepends`: `cargo`;
  optional `flatpak`
- the sibling `../pakmap-gui/PKGBUILD` publishes the desktop app separately
