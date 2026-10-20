.PHONY: build check install uninstall release help

type ?= minor
dry-run ?=
PREFIX ?= $(HOME)/.local
DESTDIR ?=

build:
	cargo build --release --locked -p codescape -p codescape-index
	@echo "built target/release/codescape and target/release/codescape-index"

install:
	@if [ "$$(id -u)" != 0 ]; then $(MAKE) --no-print-directory build; \
	elif [ ! -x target/release/codescape ] || [ ! -x target/release/codescape-index ]; then \
	echo "run make build as your user first, so target/ is not filled as root" >&2; exit 1; fi
	install -Dm755 target/release/codescape '$(DESTDIR)$(PREFIX)/bin/codescape'
	install -Dm755 target/release/codescape-index '$(DESTDIR)$(PREFIX)/bin/codescape-index'
	install -Dm644 packaging/icon/codescape.svg '$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/codescape.svg'
	tmp=$$(mktemp) && trap 'rm -f "$$tmp"' EXIT && \
	scripts/desktop-entry.sh '$(PREFIX)/bin/codescape' < packaging/linux/codescape.desktop > "$$tmp" && \
	install -Dm644 "$$tmp" '$(DESTDIR)$(PREFIX)/share/applications/codescape.desktop'
	-update-desktop-database -q '$(DESTDIR)$(PREFIX)/share/applications' 2>/dev/null
	-[ ! -f '$(DESTDIR)$(PREFIX)/share/icons/hicolor/icon-theme.cache' ] || gtk-update-icon-cache -q -t '$(DESTDIR)$(PREFIX)/share/icons/hicolor' 2>/dev/null
	@echo "installed into $(DESTDIR)$(PREFIX); CodeScape is in the application menu with its icon"

uninstall:
	rm -f '$(DESTDIR)$(PREFIX)/bin/codescape' '$(DESTDIR)$(PREFIX)/bin/codescape-index'
	rm -f '$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/codescape.svg' '$(DESTDIR)$(PREFIX)/share/applications/codescape.desktop'
	-update-desktop-database -q '$(DESTDIR)$(PREFIX)/share/applications' 2>/dev/null
	-[ ! -f '$(DESTDIR)$(PREFIX)/share/icons/hicolor/icon-theme.cache' ] || gtk-update-icon-cache -q -t '$(DESTDIR)$(PREFIX)/share/icons/hicolor' 2>/dev/null

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --release --locked -- -D warnings
	cargo test --workspace --release --locked

# Usage: make release [type=patch|minor|major] [dry-run=true]
release:
	@scripts/release.sh $(type) $(dry-run)

help:
	@echo "make build      Build codescape and codescape-index into target/release"
	@echo "make install    Build and install the apps, desktop entry and icon under DESTDIR/PREFIX (default ~/.local)"
	@echo "make uninstall  Remove what make install put under PREFIX"
	@echo "make check      Formatting, clippy and the tests, as CI runs them"
	@echo "make release    Check, bump the version, commit, tag and push from master (type=patch|minor|major, dry-run=true)"
