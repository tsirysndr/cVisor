# Alpine image providing the `cvisor` CLI and the `cvisord` daemon.
#
# Build:  docker build -t cvisor .
#
# Run the CLI:
#   docker run --rm -it --security-opt seccomp=unconfined cvisor            # interactive shell
#   docker run --rm --security-opt seccomp=unconfined cvisor -- uname -a    # run a command
#
# Run the daemon (gRPC :50051, GraphQL :8080) with ports published:
#   docker run --rm --security-opt seccomp=unconfined \
#     -p 50051:50051 -p 8080:8080 -e CVISOR_TOKEN=change-me \
#     --entrypoint cvisord cvisor
#
# cVisor installs its own seccomp filter, so the container must run with the
# default seccomp profile disabled (--security-opt seccomp=unconfined).

# Build the web UI first (the CLI embeds ui/dist via rust-embed).
FROM oven/bun:alpine AS web
WORKDIR /src/ui
COPY ui/ .
RUN bun install && bun run build

FROM rust:alpine AS build
# gcc/musl-dev build the C deps (zstd, and ring via the s3 backend's TLS)
# natively for musl; perl is needed by ring's build. protobuf provides a musl
# `protoc` for cvisor-proto's build.rs (the vendored protoc is glibc-only).
RUN apk add --no-cache musl-dev gcc make perl protobuf
WORKDIR /src
COPY . .
# Overlay the built web assets so the embedded UI isn't the placeholder.
COPY --from=web /src/ui/dist ui/dist
# rust:alpine builds natively for musl. Override the repo's rust-lld linker
# (set in .cargo/config.toml for the cross-from-macOS flow) with Alpine's gcc,
# which finds libgcc_s — needed to link host proc-macros pulled in by the s3
# feature. Built with all optional features (zstd + s3) enabled.
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=gcc
RUN cargo build -p cvisor-cli --bin cvisor --release --features zstd,s3
RUN cargo build -p cvisor-daemon --bin cvisord --release --features zstd,s3

FROM alpine:3.20
# Tools available to sandboxed commands (busybox provides /bin/sh, which the
# guest execs). Add anything your workloads need here.
COPY --from=build /src/target/release/cvisor /usr/local/bin/cvisor
COPY --from=build /src/target/release/cvisord /usr/local/bin/cvisord
# gRPC and GraphQL, respectively (the daemon binds 0.0.0.0 by default).
EXPOSE 50051 8080
ENTRYPOINT ["cvisor"]
