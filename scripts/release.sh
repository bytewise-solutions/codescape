#!/usr/bin/env bash
set -euo pipefail

type=${1:-minor}
dry_run=${2:-}

fail() { echo "error: $*" >&2; exit 1; }

cd "$(git rev-parse --show-toplevel)"
[ "$(git branch --show-current)" = master ] || fail "release from master"
[ -z "$(git status --porcelain)" ] || fail "the working tree has uncommitted changes"
git fetch -q origin master --tags
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/master)" ] || fail "master differs from origin/master; pull or push first"

current=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
IFS=. read -r major minor patch <<<"$current"
case $type in
  major) next="$((major + 1)).0.0" ;;
  minor) next="$major.$((minor + 1)).0" ;;
  patch) next="$major.$minor.$((patch + 1))" ;;
  *) fail "type must be major, minor or patch" ;;
esac
tag="v$next"
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && fail "$tag already exists"

previous=$(git describe --tags --abbrev=0 2>/dev/null || true)
changes=$(git log --no-merges --format='- %s' ${previous:+"$previous"..}HEAD)
message="Release $tag"$'\n\n'"$changes"

if [ -n "$dry_run" ]; then
  echo "would release $current -> $next as $tag with this commit message:"
  echo
  echo "$message"
  exit 0
fi

cargo fmt --all --check
cargo clippy --workspace --all-targets --release --locked -- -D warnings
cargo test --workspace --release --locked

sed -i "0,/^version = \"$current\"/s//version = \"$next\"/" Cargo.toml
cargo update --workspace --offline -q
git commit -q -am "$message"
git tag -a "$tag" -m "Release $tag"
git push --atomic origin master "$tag"
echo "pushed $tag; the Release workflow builds and publishes it"
