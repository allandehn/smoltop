#!/usr/bin/env bash
# Install a released smoltop binary. No sudo.
#
#   curl -fsSL https://raw.githubusercontent.com/allandehn/smoltop/main/install.sh | bash
#
# SMOLTOP_VERSION=X.Y.Z selects a release instead of the latest.
# SMOLTOP_INSTALL_DIR overrides ~/.local/bin.

set -euo pipefail

if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
  echo "smoltop: this installer supports x86_64 Linux only (found $(uname -s) $(uname -m))" >&2
  exit 1
fi

if [ -z "${HOME:-}" ]; then
  echo "smoltop: HOME is not set" >&2
  exit 1
fi

repo="allandehn/smoltop"
if [ -n "${SMOLTOP_VERSION:-}" ]; then
  version="${SMOLTOP_VERSION#v}"
else
  if ! effective="$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/${repo}/releases/latest")"; then
    echo "smoltop: could not look up the latest release" >&2
    exit 1
  fi
  version="${effective##*/}"
  version="${version#v}"
fi

if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "smoltop: version '${version}' is not X.Y.Z" >&2
  exit 1
fi

archive="smoltop-${version}-x86_64-linux.tar.gz"
base="https://github.com/${repo}/releases/download/v${version}"
case "$base" in
  https://*) ;;
  *)
    echo "smoltop: refusing non-https download" >&2
    exit 1
    ;;
esac

tmp="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp"
}
trap cleanup EXIT

echo "downloading ${archive}"
curl -fsSL "${base}/${archive}" -o "${tmp}/${archive}"
curl -fsSL "${base}/${archive}.sha256" -o "${tmp}/${archive}.sha256"

if ! (cd "$tmp" && sha256sum -c "${archive}.sha256"); then
  echo "smoltop: checksum mismatch; not installing" >&2
  exit 1
fi

tar -C "$tmp" -xzf "${tmp}/${archive}"
if [ ! -f "${tmp}/smoltop" ]; then
  echo "smoltop: archive has no smoltop binary" >&2
  exit 1
fi

install_dir="${SMOLTOP_INSTALL_DIR:-${HOME}/.local/bin}"
mkdir -p "$install_dir"
install -m 0755 "${tmp}/smoltop" "${install_dir}/smoltop"

echo "installed ${install_dir}/smoltop"
case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *) echo "warning: ${install_dir} is not on PATH" >&2 ;;
esac
echo "smolvm must be installed separately and on PATH (target CLI v1.32.3)."
