#!/bin/sh
# Cargo runner for aarch64-unknown-linux-musl test/bench binaries.
# Cargo invokes this with the built binary path as $1 and any test args after.
# We bind-mount the binary's directory into an Alpine container and run it there
# with seccomp disabled (the sandbox installs its own seccomp filter).
set -eu

bin="$1"
shift

bin_dir="$(cd "$(dirname "$bin")" && pwd)"
bin_name="$(basename "$bin")"

exec docker run --rm --security-opt seccomp=unconfined \
    -e BVISOR_DEBUG \
    -v "$bin_dir:/t" \
    alpine "/t/$bin_name" "$@"
