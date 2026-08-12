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

# seccomp=unconfined: the sandbox installs its own seccomp filter.
# apparmor=unconfined: the supervisor uses ptrace-class ops (pidfd_getfd,
#   process_vm_readv) against the guest, which the docker-default AppArmor
#   profile can restrict on real Linux hosts (e.g. GitHub Actions).
exec docker run --rm \
    --security-opt seccomp=unconfined \
    --security-opt apparmor=unconfined \
    -e BVISOR_DEBUG \
    -v "$bin_dir:/t" \
    alpine "/t/$bin_name" "$@"
