#!/usr/bin/env bash
# Publish gleam_cvisor to Hex.
#
# The committed gleam.toml uses a PATH dependency on ../erlang so `gleam test`
# works against the in-repo Erlang runtime. Hex rejects path deps in a published
# package, so this script temporarily swaps the dependency to the Hex release,
# runs `gleam publish`, and restores the originals (even on failure).
#
# Usage:  bin/publish.sh [--yes]   (extra args are forwarded to `gleam publish`)
set -euo pipefail

# The Hex version constraint for the published package. Bump alongside the
# Erlang `cvisor` release the SDK targets.
HEX_REQ='cvisor = ">= 0.3.0 and < 1.0.0"'
PATH_REQ='cvisor = { path = "../erlang" }'

cd "$(dirname "$0")/.."

restore() {
  [ -f gleam.toml.bak ] && mv gleam.toml.bak gleam.toml
  [ -f manifest.toml.bak ] && mv manifest.toml.bak manifest.toml
}
trap restore EXIT

if ! grep -qF "$PATH_REQ" gleam.toml; then
  echo "error: expected path dep line not found in gleam.toml:" >&2
  echo "  $PATH_REQ" >&2
  exit 1
fi

cp gleam.toml gleam.toml.bak
[ -f manifest.toml ] && cp manifest.toml manifest.toml.bak

# Swap the path dep for the Hex release and drop the manifest so `gleam publish`
# re-resolves against Hex.
sed -i.tmp "s|$PATH_REQ|$HEX_REQ|" gleam.toml && rm -f gleam.toml.tmp
rm -f manifest.toml

echo "Publishing with: $HEX_REQ"
gleam publish "$@"
