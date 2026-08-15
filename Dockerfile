# Alpine image providing the `cvisor` CLI.
#
# Build:  docker build -t cvisor .
# Run:    docker run --rm -it --security-opt seccomp=unconfined cvisor            # interactive shell
#         docker run --rm --security-opt seccomp=unconfined cvisor -- uname -a    # run a command
#
# cVisor installs its own seccomp filter, so the container must run with the
# default seccomp profile disabled (--security-opt seccomp=unconfined).

FROM rust:alpine AS build
# gcc/musl-dev build the C deps (zstd, and ring via the s3 backend's TLS)
# natively for musl; perl is needed by ring's build.
RUN apk add --no-cache musl-dev gcc make perl
WORKDIR /src
COPY . .
# rust:alpine builds natively for musl. Override the repo's rust-lld linker
# (set in .cargo/config.toml for the cross-from-macOS flow) with Alpine's gcc,
# which finds libgcc_s — needed to link host proc-macros pulled in by the s3
# feature. Built with all optional features (zstd + s3) enabled.
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=gcc
RUN cargo build -p cvisor-core --bin cvisor --release --features zstd,s3

FROM alpine:3.20
# Tools available to sandboxed commands (busybox provides /bin/sh, which the
# CLI execs). Add anything your workloads need here.
COPY --from=build /src/target/release/cvisor /usr/local/bin/cvisor
ENTRYPOINT ["cvisor"]
