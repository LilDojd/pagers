#!/usr/bin/env bash
set -euo pipefail

if [ -z "${1:-}" ]; then
	echo "Please provide a tag."
	echo "Usage: ./release.sh v[X.Y.Z]"
	exit 1
fi

TAG="$1"
VERSION="${TAG#v}"

if ! command -v typos &>/dev/null; then
	echo "typos is not installed. Enter devenv shell or run 'cargo install typos-cli'."
fi

echo "Preparing ${TAG}..."

# check for typos
typos || true

# update the version in all crate Cargo.toml files
for toml in crates/*/Cargo.toml; do
	sed -i -E "s/^version = \"[^\"]+\"/version = \"${VERSION}\"/" "$toml"
done

# update the changelog
git cliff --config cliff.toml --tag "$TAG" -o CHANGELOG.md

# commit the release prep
jj desc -m "chore(release): prepare for ${TAG}"
jj new

# export jj state to git, then create a git tag on the release commit
jj bookmark move main --to @-
jj git export
git tag -a "$TAG" -m "Release $TAG" main

echo "Done!"
echo "Now push the commit and tag:"
echo "  jj git push -b main"
echo "  git push origin ${TAG}"
