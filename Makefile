.PHONY: build check release help

type ?= minor
dry-run ?=

build:
	cargo build --release --locked -p codescape -p codescape-index
	@echo "built target/release/codescape and target/release/codescape-index"

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --release --locked -- -D warnings
	cargo test --workspace --release --locked

# Usage: make release [type=patch|minor|major] [dry-run=true]
release:
	@scripts/release.sh $(type) $(dry-run)

help:
	@echo "make build      Build codescape and codescape-index into target/release"
	@echo "make check      Formatting, clippy and the tests, as CI runs them"
	@echo "make release    Check, bump the version, commit, tag and push from master (type=patch|minor|major, dry-run=true)"
