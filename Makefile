.PHONY: build check install uninstall release help

type ?= minor
dry-run ?=
PREFIX ?= $(HOME)/.local

build:
	cargo build --release --locked -p codescape -p codescape-index
	@echo "built target/release/codescape and target/release/codescape-index"

install: build
	install -Dm755 target/release/codescape $(PREFIX)/bin/codescape
	install -Dm755 target/release/codescape-index $(PREFIX)/bin/codescape-index
	install -Dm644 packaging/icon/codescape.svg $(PREFIX)/share/icons/hicolor/scalable/apps/codescape.svg
	sed 's|^Exec=codescape|Exec=$(PREFIX)/bin/codescape|' packaging/linux/codescape.desktop > codescape.desktop.tmp
	install -Dm644 codescape.desktop.tmp $(PREFIX)/share/applications/codescape.desktop
	rm codescape.desktop.tmp
	-update-desktop-database -q $(PREFIX)/share/applications 2>/dev/null
	@echo "installed into $(PREFIX); CodeScape is in the application menu with its icon"

uninstall:
	rm -f $(PREFIX)/bin/codescape $(PREFIX)/bin/codescape-index
	rm -f $(PREFIX)/share/icons/hicolor/scalable/apps/codescape.svg $(PREFIX)/share/applications/codescape.desktop

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --release --locked -- -D warnings
	cargo test --workspace --release --locked

# Usage: make release [type=patch|minor|major] [dry-run=true]
release:
	@scripts/release.sh $(type) $(dry-run)

help:
	@echo "make build      Build codescape and codescape-index into target/release"
	@echo "make install    Build and install the apps, desktop entry and icon under PREFIX (default ~/.local)"
	@echo "make uninstall  Remove what make install put under PREFIX"
	@echo "make check      Formatting, clippy and the tests, as CI runs them"
	@echo "make release    Check, bump the version, commit, tag and push from master (type=patch|minor|major, dry-run=true)"
