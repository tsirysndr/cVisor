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

# seccomp=unconfined: the sandbox installs its own seccomp filter.
# apparmor=unconfined: the supervisor uses ptrace-class ops (pidfd_getfd,
#   process_vm_readv) against the guest, which the docker-default AppArmor
#   profile can restrict on real Linux hosts (e.g. GitHub Actions).
exec docker run --rm \
    --security-opt seccomp=unconfined \
    --security-opt apparmor=unconfined \
    --platform linux/amd64 \
    -v "$bin_dir:/t" \
    alpine "/t/$bin_name" "$@"
