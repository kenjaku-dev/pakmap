# pakmap — butter Makefile
PREFIX ?= /usr/local

.PHONY: help build release install test fmt clippy clean run gui-dev gui-release gui-e2e install-gui install-gui-assets uninstall-gui

help:  ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

build: ## Debug build
	cargo build

release: ## Optimized release build (z + lto + strip)
	cargo build --release
	@ls -lh target/release/pakmap

install: release ## Install to ~/.cargo/bin and /usr/local/bin (if sudo)
	cargo install --path . --force
	@echo "Installed to ~/.cargo/bin/pakmap"
	@which pakmap && pakmap --version || true

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

install-gui-assets: ## .desktop + icons only (AUR package() reuses this)
	install -Dm644 gui/desktop/pakmap-gui.desktop $(DESTDIR)$(PREFIX)/share/applications/pakmap-gui.desktop
	install -Dm644 gui/src-tauri/icons/32x32.png      $(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps/pakmap-gui.png
	install -Dm644 gui/src-tauri/icons/128x128.png    $(DESTDIR)$(PREFIX)/share/icons/hicolor/128x128/apps/pakmap-gui.png
	install -Dm644 gui/src-tauri/icons/128x128@2x.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/pakmap-gui.png

uninstall-gui: ## Remove GUI binary + .desktop + icons (same DESTDIR/PREFIX)
	rm -f $(DESTDIR)$(PREFIX)/bin/pakmap-gui
	rm -f $(DESTDIR)$(PREFIX)/share/applications/pakmap-gui.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/icons/hicolor/{32x32,128x128,256x256}/apps/pakmap-gui.png

# butter aliases
dev: fmt clippy build ## fmt+clippy+build in one go
update-db: ## Refresh pacman DB (needed for repo search)
	sudo pacman -Sy
