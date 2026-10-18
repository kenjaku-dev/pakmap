# pakmap-gui — AUR publishing

`PKGBUILD` here is the source of truth (it mirrors `make install-gui-assets`).
Publishing steps, per release:

1. `pkgver=…` + `pkgrel=1`
2. Real checksum: `updpkgsums` (or `makepkg -g >> PKGBUILD`) once the tag exists
3. `namcap PKGBUILD` if available
4. `makepkg --printsrcinfo > .SRCINFO`
5. Copy `PKGBUILD` + `.SRCINFO` to the `pakmap-gui` AUR git repo, commit, push

Notes:
- build runs `pnpm install --frozen-lockfile && pnpm build` then
  `cargo build --release --locked -p pakmap-gui --features tauri/custom-protocol` against the GitHub tag tarball
- `check()` runs vitest only (Playwright e2e stays a repo/CI gate)
- deps: `webkit2gtk-4.1 gtk3 polkit pacman git pacman-contrib`;
  `makedepends`: `cargo nodejs pnpm`; optional `flatpak`
- privileged flows use `pkexec` at runtime (polkit dialog = the confirmation),
  pacman always runs `--noconfirm` in the GUI — see
  `docs/gui-architecture.md` §privilege model for the rationale
