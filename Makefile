.PHONY: release help

type ?= minor
dry-run ?=

# Usage: make release [type=patch|minor|major] [dry-run=true]
release:
	@scripts/release.sh $(type) $(dry-run)

help:
	@echo "make release [type=patch|minor|major] [dry-run=true]"
	@echo "  Check, bump the version, commit, tag and push from master; CI then builds and publishes the release."
