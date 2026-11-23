# pakmap — butter Makefile
PREFIX ?= /usr/local
# Override for hermetic tests: make install-assets PAKMAP_BIN=<fixture script>
PAKMAP_BIN ?= target/release/pakmap

.PHONY: help build release install install-assets test fmt clippy clean run aur-guard gui-dev gui-release gui-e2e install-gui install-gui-assets uninstall-gui

help:  ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

build: ## Debug build
	cargo build

release: ## Optimized release build (z + lto + strip)
	cargo build --release
	@ls -lh target/release/pakmap

install: release install-assets ## Install the CLI binary + completions/man (DESTDIR/PREFIX aware for assets)
	cargo install --path . --force
	@echo "Installed to ~/.cargo/bin/pakmap"
	@which pakmap && pakmap --version || true

install-assets: ## Generate + install bash/zsh/fish completions and the man page from the CLI parser
	@test -x $(PAKMAP_BIN) || { echo "$(PAKMAP_BIN) not built — run 'make release' first"; exit 1; }
	@tmp=$$(mktemp -d); trap 'rm -rf "$$tmp"' EXIT; \
	  $(PAKMAP_BIN) --generate-completions bash > "$$tmp/pakmap" && \
	  $(PAKMAP_BIN) --generate-completions zsh  > "$$tmp/_pakmap" && \
	  $(PAKMAP_BIN) --generate-completions fish > "$$tmp/pakmap.fish" && \
	  $(PAKMAP_BIN) --generate-man              > "$$tmp/pakmap.1" && \
	  install -Dm644 "$$tmp/pakmap"      $(DESTDIR)$(PREFIX)/share/bash-completion/completions/pakmap && \
	  install -Dm644 "$$tmp/_pakmap"     $(DESTDIR)$(PREFIX)/share/zsh/site-functions/_pakmap && \
	  install -Dm644 "$$tmp/pakmap.fish" $(DESTDIR)$(PREFIX)/share/fish/vendor_completions.d/pakmap.fish && \
	  install -Dm644 "$$tmp/pakmap.1"    $(DESTDIR)$(PREFIX)/share/man/man1/pakmap.1

test: ## Run all checks + live search smoke test
	cargo fmt --check
	cargo clippy -- -D warnings
	cargo test 2>&1 | head -n 50
	./target/debug/pakmap neovim --limit 2 --json | head -n 20
	./target/debug/pakmap firefox --source repo --limit 2

fmt: ## Format code
	cargo fmt

clippy: ## Lint
	cargo clippy

check: ## cargo check — fast dev-loop feedback (no codegen, no binary)
	cargo check

aur-guard: ## Fail-closed AUR publish pre-flight: make aur-guard V=v0.12.0
	scripts/aur-publish-guard.sh "$(V)"

timings: ## debug build with the timing report -> target/cargo-timings/cargo-timing.html
	cargo build --timings

clean: ## Clean target
	cargo clean

run: ## Quick run, e.g. make run Q=firefox
	cargo run -- $(Q) --limit 5

gui-dev: ## Run the desktop GUI in dev mode (hot reload)
	cd gui && pnpm tauri dev

gui-release: ## Release build of the GUI binary (dist/ + cargo, assets embedded via tauri/custom-protocol)
	cd gui && pnpm install --frozen-lockfile
	cd gui && pnpm build
	cargo build --release --locked -p pakmap-gui --features tauri/custom-protocol
	@ls -lh target/release/pakmap-gui

gui-e2e: ## GUI battery (vitest axe/e2e via Playwright, mock gateway)
	cd gui && pnpm test
	cd gui && pnpm exec playwright test

install-gui: ## Install GUI + .desktop + hicolor icons (DESTDIR/PREFIX aware). Run `make gui-release` first, as your user
	@test -x target/release/pakmap-gui || { \
	  echo "target/release/pakmap-gui not built yet."; \
	  echo "Run 'make gui-release' as your user first — NOT under sudo:"; \
	  echo "sudo strips PATH (~/.local/bin), so the pnpm build cannot run there."; \
	  exit 1; \
	}
	install -Dm755 target/release/pakmap-gui $(DESTDIR)$(PREFIX)/bin/pakmap-gui
	$(MAKE) install-gui-assets DESTDIR=$(DESTDIR) PREFIX=$(PREFIX)
	@echo "Installed pakmap-gui to $(DESTDIR)$(PREFIX)/bin"

install-gui-assets: ## .desktop + icons + AppStream metadata (AUR package() reuses this)
	install -Dm644 gui/desktop/pakmap-gui.desktop $(DESTDIR)$(PREFIX)/share/applications/pakmap-gui.desktop
	install -Dm644 gui/desktop/pakmap-gui.metainfo.xml $(DESTDIR)$(PREFIX)/share/metainfo/pakmap-gui.metainfo.xml
	install -Dm644 gui/desktop/screenshots/pakmap-search.png  $(DESTDIR)$(PREFIX)/share/doc/pakmap-gui/screenshots/pakmap-search.png
	install -Dm644 gui/desktop/screenshots/pakmap-updates.png $(DESTDIR)$(PREFIX)/share/doc/pakmap-gui/screenshots/pakmap-updates.png
	install -Dm644 gui/src-tauri/icons/32x32.png      $(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps/pakmap-gui.png
	install -Dm644 gui/src-tauri/icons/128x128.png    $(DESTDIR)$(PREFIX)/share/icons/hicolor/128x128/apps/pakmap-gui.png
	install -Dm644 gui/src-tauri/icons/128x128@2x.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/pakmap-gui.png

uninstall-gui: ## Remove GUI binary + .desktop + icons + metadata (same DESTDIR/PREFIX)
	rm -f $(DESTDIR)$(PREFIX)/bin/pakmap-gui
	rm -f $(DESTDIR)$(PREFIX)/share/applications/pakmap-gui.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/metainfo/pakmap-gui.metainfo.xml
	rm -rf $(DESTDIR)$(PREFIX)/share/doc/pakmap-gui
	rm -f $(DESTDIR)$(PREFIX)/share/icons/hicolor/{32x32,128x128,256x256}/apps/pakmap-gui.png

# butter aliases
dev: fmt clippy check ## fmt+clippy+check in one go (fast loop; `make build` only when a binary is needed)
update-db: ## Refresh pacman DB (needed for repo search)
	sudo pacman -Sy
