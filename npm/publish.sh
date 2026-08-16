#!/usr/bin/env bash
# Publish @cvisor/cli and its per-host binary packages to npm, from a machine
# logged in with `npm login`. Run it after the release workflow has attached the
# tarballs to the GitHub Release for the tag:
#
#   ./npm/publish.sh v0.2.0            # download the release assets, then publish
#   ./npm/publish.sh v0.2.0 --dry-run  # stage and pack, publish nothing
set -euo pipefail

tag="${1:-}"
dry_run="${2:-}"
if [[ -z $tag ]]; then
  echo "usage: $0 <tag> [--dry-run]   e.g. $0 v0.2.0" >&2
  exit 1
fi
version="${tag#v}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "==> downloading $tag release assets"
gh release download "$tag" --dir "$work" --pattern '*.tar.gz'

echo "==> unpacking binaries into the platform packages"
mkdir -p npm/linux-x64/bin npm/linux-arm64/bin npm/darwin-arm64/bin
tar -xzf "$work/cvisor-linux-x86_64.tar.gz" -C npm/linux-x64/bin
tar -xzf "$work/cvisor-linux-aarch64.tar.gz" -C npm/linux-arm64/bin
tar -xzf "$work/cvisor-darwin-aarch64.tar.gz" -C npm/darwin-arm64/bin
chmod +x npm/linux-x64/bin/* npm/linux-arm64/bin/* npm/darwin-arm64/bin/*

# The tag drives the published version; the committed 0.1.0 in each
# package.json is a placeholder, so this leaves them dirty — `git checkout npm`
# afterwards, or commit the bump if you keep them in sync with releases.
echo "==> stamping version $version"
node npm/version.mjs "$version"

# The platform packages go first: the launcher pins their exact version, so
# publishing it earlier would leave a window where it cannot resolve.
for dir in linux-x64 linux-arm64 darwin-arm64 cli; do
  name="$(node -p "require('./npm/$dir/package.json').name")"
  if npm view "$name@$version" version >/dev/null 2>&1; then
    echo "==> $name@$version is already on npm, skipping"
  elif [[ $dry_run == "--dry-run" ]]; then
    echo "==> would publish $name@$version"
    # ./-prefixed, else npm reads the path as a `user/repo` spec.
    npm pack --dry-run "./npm/$dir"
  else
    echo "==> publishing $name@$version"
    npm publish "./npm/$dir" --access public
  fi
done
