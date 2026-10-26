#!/usr/bin/env bash
# Release smoltop.
#
#   scripts/release.sh X.Y.Z --dry-run
#   scripts/release.sh X.Y.Z
#
# Preflight always runs: clean tree, branch main, fmt, clippy, tests.
# Dry-run prints the version bump, build, tag, and GitHub release it would
# perform, then exits. It must not edit the tree.
# Without --dry-run, the script bumps Cargo.toml, builds --release, commits,
# tags, pushes main and the tag, and opens a GitHub release. The first
# failure stops the script.

set -euo pipefail

cd "$(dirname -- "$0")/.."

version=""
dry_run=0

usage() {
  echo "usage: scripts/release.sh X.Y.Z [--dry-run]" >&2
  exit 2
}

for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    -h|--help) usage ;;
    *)
      if [[ -n "$version" ]]; then
        echo "unexpected argument: $arg" >&2
        exit 2
      fi
      version="$arg"
      ;;
  esac
done

if [[ -z "$version" ]]; then
  usage
fi

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "version must look like X.Y.Z (got: $version)" >&2
  exit 2
fi

echo "==> preflight"

if [[ -n "$(git status --porcelain)" ]]; then
  echo "working tree is not clean" >&2
  git status --porcelain >&2
  exit 1
fi

branch="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$branch" != "main" ]]; then
  echo "must be on main (currently ${branch})" >&2
  exit 1
fi

echo "==> cargo fmt --check"
cargo fmt --all -- --check

echo "==> cargo clippy"
cargo clippy --locked --all-targets -- -D warnings

echo "==> cargo test"
cargo test --locked

tag="v${version}"

if [[ "$dry_run" -eq 1 ]]; then
  echo "dry-run: would set Cargo.toml version to ${version}"
  echo "dry-run: would run cargo build --release"
  echo "dry-run: would commit Cargo.toml and Cargo.lock"
  echo "dry-run: would tag ${tag}"
  echo "dry-run: would push origin main and ${tag}"
  echo "dry-run: would create GitHub release ${tag}"
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "dry-run changed the tree; that is a bug" >&2
    git status --porcelain >&2
    exit 1
  fi
  echo "dry-run: no changes made"
  exit 0
fi

if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "tag ${tag} already exists" >&2
  exit 1
fi

echo "==> bump version to ${version}"
tmp="$(mktemp)"
awk -v ver="$version" '
  BEGIN { done = 0 }
  /^version = "/ && done == 0 {
    print "version = \"" ver "\""
    done = 1
    next
  }
  { print }
  END { if (done != 1) exit 1 }
' Cargo.toml >"$tmp"
mv "$tmp" Cargo.toml

echo "==> cargo build --release"
# Not --locked: the version bump has to land in Cargo.lock before the commit.
cargo build --release

echo "==> commit"
git add Cargo.toml Cargo.lock
git commit -m "Release ${version}"

echo "==> tag ${tag}"
git tag -a "$tag" -m "smoltop ${tag}"

echo "==> push main and ${tag}"
git push origin main
git push origin "$tag"

echo "==> GitHub release ${tag}"
gh release create "$tag" --title "smoltop ${tag}" --notes "smoltop ${tag}"

echo "released ${tag}"
