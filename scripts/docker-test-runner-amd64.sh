#!/bin/sh
# Cargo runner for x86_64-unknown-linux-musl test/bench binaries.
# Same as docker-test-runner.sh but forces linux/amd64 so it works on an
# arm64 host under emulation (unit tests use mocks; real-seccomp tests are
# limited under qemu, matching the old `zig build test --platform` behavior).
set -eu

bin="$1"
shift

bin_dir="$(cd "$(dirname "$bin")" && pwd)"
bin_name="$(basename "$bin")"

exec docker run --rm --security-opt seccomp=unconfined \
    --platform linux/amd64 \
    -v "$bin_dir:/t" \
    alpine "/t/$bin_name" "$@"
