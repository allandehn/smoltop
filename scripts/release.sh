#!/usr/bin/env bash
# Release smoltop. main is protected: this script never pushes main.
#
#   scripts/release.sh X.Y.Z [--dry-run]
#     From a clean, up-to-date main: cut branch release/X.Y.Z, bump
#     Cargo.toml and Cargo.lock, commit, and push only that branch so it
#     can be opened as a pull request.
#
#   scripts/release.sh X.Y.Z --publish [--dry-run]
#     After that pull request is merged, on up-to-date main: check the
#     version already matches, build --release for x86_64-unknown-linux-gnu,
#     write smoltop-X.Y.Z-x86_64-linux.tar.gz (binary, LICENSE, README) and
#     a .sha256 file, tag vX.Y.Z, push only the tag, and attach both files
#     with gh release create.
#
# --dry-run prints every step and must not edit the tree, commit, push,
# tag, or call GitHub. The first real failure stops the script.

set -euo pipefail

cd "$(dirname -- "$0")/.." || exit 1

version=""
dry_run=0
publish=0

usage() {
  echo "usage: scripts/release.sh X.Y.Z [--dry-run]" >&2
  echo "       scripts/release.sh X.Y.Z --publish [--dry-run]" >&2
  exit 2
}

for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    --publish) publish=1 ;;
    -h | --help) usage ;;
    *)
      if [ -n "$version" ]; then
        echo "unexpected argument: $arg" >&2
        exit 2
      fi
      version="$arg"
      ;;
  esac
done

if [ -z "$version" ]; then
  usage
fi

if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "version must look like X.Y.Z (got: $version)" >&2
  exit 2
fi

tag="v${version}"
release_branch="release/${version}"
archive="smoltop-${version}-x86_64-linux.tar.gz"
target_triple="x86_64-unknown-linux-gnu"

# Refuse any ref that would update main. Call this before every git push.
refuse_main_push() {
  case "$1" in
    main | refs/heads/main)
      echo "refusing to push main" >&2
      exit 1
      ;;
  esac
}

cargo_version() {
  awk '
    /^version = "/ {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' Cargo.toml
}

require_clean_main() {
  echo "==> preflight"

  if [ -n "$(git status --porcelain)" ]; then
    echo "working tree is not clean" >&2
    git status --porcelain >&2
    exit 1
  fi

  branch="$(git rev-parse --abbrev-ref HEAD)"
  if [ "$branch" != "main" ]; then
    echo "must be on main (currently ${branch})" >&2
    exit 1
  fi

  if ! git rev-parse --verify --quiet origin/main >/dev/null; then
    echo "origin/main is missing; fetch it first" >&2
    exit 1
  fi
  local_main="$(git rev-parse HEAD)"
  remote_main="$(git rev-parse origin/main)"
  if [ "$local_main" != "$remote_main" ]; then
    echo "main is not up to date with origin/main" >&2
    exit 1
  fi

  echo "==> cargo fmt --check"
  cargo fmt --all -- --check

  echo "==> cargo clippy"
  cargo clippy --locked --all-targets -- -D warnings

  echo "==> cargo test"
  cargo test --locked
}

assert_dry_run_clean() {
  if [ -n "$(git status --porcelain)" ]; then
    echo "dry-run changed the tree; that is a bug" >&2
    git status --porcelain >&2
    exit 1
  fi
  echo "dry-run: no changes made"
}

bump_version_file() {
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
}

prepare_bump() {
  require_clean_main
  current="$(cargo_version)"
  if [ "$current" = "$version" ]; then
    echo "Cargo.toml is already ${version}; nothing to bump" >&2
    exit 1
  fi
  if git show-ref --verify --quiet "refs/heads/${release_branch}"; then
    echo "branch ${release_branch} already exists" >&2
    exit 1
  fi
}

run_bump() {
  prepare_bump
  if [ "$dry_run" -eq 1 ]; then
    echo "dry-run: would create branch ${release_branch} from main"
    echo "dry-run: would set Cargo.toml version to ${version}"
    echo "dry-run: would refresh Cargo.lock with cargo check"
    echo "dry-run: would commit Cargo.toml and Cargo.lock"
    echo "dry-run: would push origin ${release_branch}"
    echo "dry-run: would not push main, tag, or create a GitHub release"
    echo "dry-run: open a pull request from ${release_branch} into main"
    assert_dry_run_clean
    exit 0
  fi

  echo "==> branch ${release_branch}"
  git switch -c "$release_branch"

  echo "==> bump Cargo.toml to ${version}"
  bump_version_file

  echo "==> refresh Cargo.lock"
  # Not --locked: the new package version has to land in the lockfile.
  cargo check

  echo "==> commit"
  git add Cargo.toml Cargo.lock
  git commit -m "Release ${version}"

  echo "==> push ${release_branch}"
  refuse_main_push "$release_branch"
  git push -u origin "$release_branch"

  echo "pushed ${release_branch}; open a pull request into main"
  echo "main was not pushed"
}

prepare_publish() {
  require_clean_main
  current="$(cargo_version)"
  if [ "$current" != "$version" ]; then
    echo "Cargo.toml version is ${current}, not ${version}" >&2
    exit 1
  fi
  if git rev-parse --verify --quiet "refs/tags/${tag}" >/dev/null; then
    echo "tag ${tag} already exists" >&2
    exit 1
  fi
  if git ls-remote --tags origin "refs/tags/${tag}" | grep -q .; then
    echo "tag ${tag} already exists on origin" >&2
    exit 1
  fi
}

package_release() {
  bin="target/${target_triple}/release/smoltop"
  if [ ! -f "$bin" ]; then
    echo "missing release binary ${bin}" >&2
    exit 1
  fi
  dist="target/dist"
  stage="$(mktemp -d)"
  mkdir -p "$dist"
  cp "$bin" "${stage}/smoltop"
  cp LICENSE "${stage}/LICENSE"
  cp README.md "${stage}/README"
  tar -C "$stage" -czf "${dist}/${archive}" smoltop LICENSE README
  rm -rf "$stage"
  (
    cd "$dist"
    sha256sum "$archive" >"${archive}.sha256"
  )
  echo "wrote ${dist}/${archive}"
  echo "wrote ${dist}/${archive}.sha256"
}

run_publish() {
  prepare_publish
  if [ "$dry_run" -eq 1 ]; then
    echo "dry-run: Cargo.toml version ${version} matches"
    echo "dry-run: would run cargo build --release --locked --target ${target_triple}"
    echo "dry-run: would write target/dist/${archive} containing smoltop, LICENSE, and README"
    echo "dry-run: would write target/dist/${archive}.sha256"
    echo "dry-run: would tag ${tag}"
    echo "dry-run: would push only the tag ${tag}"
    echo "dry-run: would not push main"
    echo "dry-run: would run gh release create ${tag} with ${archive} and ${archive}.sha256"
    assert_dry_run_clean
    exit 0
  fi

  echo "==> cargo build --release --target ${target_triple}"
  cargo build --release --locked --target "$target_triple"

  echo "==> package ${archive}"
  package_release

  echo "==> tag ${tag}"
  git tag -a "$tag" -m "smoltop ${tag}"

  echo "==> push tag ${tag}"
  refuse_main_push "$tag"
  git push origin "refs/tags/${tag}:refs/tags/${tag}"

  echo "==> GitHub release ${tag}"
  gh release create "$tag" \
    --title "smoltop ${tag}" \
    --notes "smoltop ${tag}" \
    "target/dist/${archive}" \
    "target/dist/${archive}.sha256"

  echo "published ${tag} (tag pushed; main was not pushed)"
}

if [ "$publish" -eq 1 ]; then
  run_publish
else
  run_bump
fi
