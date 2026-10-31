# Quick Install (ez & butter)

## 0. Automatic (recommended — installs AND updates; rerun any time)
```bash
./install.sh              # pinned GitHub release, sha256-checked when the manifest ships, CLI always
./install.sh --gui        # + desktop GUI: .desktop + hicolor icons, caches auto-refreshed
./install.sh --user       # no sudo: -> ~/.local/bin + ~/.local/share
./install.sh --latest     # newest release instead of the pinned one
./install.sh --check      # print pinned vs installed vs latest, change nothing
./install.sh --help       # all flags (--prefix/--no-sync/--yes)
```
The script carries its target version in `PAKMAP_VERSION` at the top and is
bumped in the same commit as every release, so it always matches the release
it advertises. GUI follows presence automatically: if `pakmap-gui` is already
installed it updates in place; otherwise pass `--gui` once. Desktop icons are
refreshed via `gtk-update-icon-cache` + `update-desktop-database` on every run.

## 1. One-liner build & install
```bash
cd /home/achraf/projects/pakmap
make install        # or: cargo install --path . --force
# or straight from git — name the CLI, the workspace also contains pakmap-gui:
cargo install --git https://github.com/kenjaku-dev/pakmap --locked pakmap
# manual prebuilt alternative (x86_64 Arch tarball):
curl -LO https://github.com/kenjaku-dev/pakmap/releases/download/v0.9.3/pakmap-0.9.3-x86_64-linux.tar.gz
tar xzf pakmap-0.9.3-x86_64-linux.tar.gz && sudo install -Dm755 pakmap /usr/local/bin/pakmap
pakmap --help
```

## 2. Sync pacman DB first (required for repo results)
```bash
sudo pacman -Sy
```

## 3. Try it
```bash
pakmap firefox --limit 5
pakmap neovim --json | jq
pakmap yay --source aur --by name
pakmap "linux.*headers" --regex
pakmap rust --installed-only
```

## 4. Release binary
```bash
make release
ls -lh target/release/pakmap   # ~6.1M stripped
sudo install -Dm755 target/release/pakmap /usr/local/bin/pakmap
```

## 5. Desktop GUI (v0.9.3)
```bash
make gui-release        # vite dist + release binary (assets embedded via tauri/custom-protocol)
sudo make install-gui   # -> /usr/local/bin + .desktop + hicolor icons
# AUR: pakmap-gui (git clone aur.archlinux.org/pakmap-gui && makepkg -si)
# extras: webkit2gtk-4.1, polkit; pacman ops ask via pkexec (Cancel is safe)
```

## Shell completions (optional butter)
```bash
# bash
pakmap --help | head   # helps discovery
# completions via clap_complete if you enable feature later:
# cargo run -- --generate-completion bash > /usr/share/bash-completion/completions/pakmap
```

## Update
```bash
# prebuilt installs: rerun the installer — same file installs AND updates
./install.sh                       # or: --user, --gui, --latest
# or check what would happen first:
./install.sh --check               # pinned vs installed vs latest

# source-built installs (clone/repo + toolchain): rebuild reinstall
./update.sh                        # also: --user / --gui / --check
cd /home/achraf/projects/pakmap
git pull
cargo install --path . --force
```
