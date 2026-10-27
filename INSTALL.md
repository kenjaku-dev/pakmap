# Quick Install (ez & butter)

## 0. Automatic (recommended)
```bash
./install.sh              # build + sudo install to /usr/local/bin
./install.sh --user       # or: no sudo, installs to ~/.cargo/bin
./install.sh --help       # all flags (--prefix/--yes/--no-sync)
```

## 1. One-liner build & install
```bash
cd /home/achraf/projects/pakmap
make install        # or: cargo install --path . --force
# or straight from git — name the CLI, the workspace also contains pakmap-gui:
cargo install --git https://github.com/kenjaku-dev/pakmap --locked pakmap
# or with no toolchain at all (x86_64 Arch tarball):
curl -LO https://github.com/kenjaku-dev/pakmap/releases/download/v0.9.0/pakmap-0.9.0-x86_64-linux.tar.gz
tar xzf pakmap-0.9.0-x86_64-linux.tar.gz && sudo install -Dm755 pakmap /usr/local/bin/pakmap
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

## 5. Desktop GUI (v0.9.0)
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
cd /home/achraf/projects/pakmap
git pull
cargo install --path . --force
```
