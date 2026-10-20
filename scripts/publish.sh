#!/usr/bin/env bash
set -euo pipefail

fail() { echo "error: $*" >&2; exit 1; }

cd "$(git rev-parse --show-toplevel)"
[ "$(git branch --show-current)" = master ] || fail "publish from master"
tag=$(git describe --tags --exact-match HEAD 2>/dev/null) || fail "HEAD is not tagged; run make release first"
git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null && fail "$tag is already on origin"

git push --atomic origin master "$tag"
command -v gh >/dev/null || { echo "pushed $tag; install gh to follow the Release workflow from here"; exit 0; }

run=""
for _ in $(seq 30); do
  run=$(gh run list --workflow release.yml --branch "$tag" --limit 1 --json databaseId --jq '.[0].databaseId // empty')
  [ -n "$run" ] && break
  sleep 2
done
[ -n "$run" ] || fail "no Release workflow run found for $tag"
gh run watch "$run" --exit-status
gh release view "$tag" --json url --jq .url
