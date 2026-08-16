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

# Build the web UI first (the CLI embeds ui/dist via rust-embed). Dependencies
# install in their own layer so editing ui/src doesn't re-run bun install.
FROM oven/bun:alpine AS web
WORKDIR /src/ui
COPY ui/package.json ui/bun.lock ./
RUN bun install
COPY ui/ .
RUN bun run build

# gcc/musl-dev build the C deps (zstd, and ring via the s3 backend's TLS)
# natively for musl; perl is needed by ring's build. protobuf provides a musl
# `protoc` for cvisor-proto's build.rs (the vendored protoc is glibc-only).
FROM rust:alpine AS chef
RUN apk add --no-cache musl-dev gcc make perl protobuf \
    && cargo install cargo-chef --locked
WORKDIR /src

# The recipe is a manifest-only digest of the workspace: it changes when
# dependencies change, not when sources do.
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
# rust:alpine builds natively for musl. Override the repo's rust-lld linker
# (set in .cargo/config.toml for the cross-from-macOS flow) with Alpine's gcc,
# which finds libgcc_s — needed to link host proc-macros pulled in by the s3
# feature. Built with all optional features (zstd + s3) enabled.
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=gcc
# Compile every dependency against stub sources; this layer is reused across
# source-only changes. Feature flags must match the real build below or the
# cached artifacts don't apply.
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release -p cvisor-cli --features zstd,s3 --recipe-path recipe.json \
 && cargo chef cook --release -p cvisor-daemon --features zstd,s3 --recipe-path recipe.json
COPY . .
# Overlay the built web assets so the embedded UI isn't the placeholder.
COPY --from=web /src/ui/dist ui/dist
RUN cargo build -p cvisor-cli --bin cvisor --release --features zstd,s3
RUN cargo build -p cvisor-daemon --bin cvisord --release --features zstd,s3

FROM alpine:latest
# Tools available to sandboxed commands (busybox provides /bin/sh, which the
# guest execs). Add anything else your workloads need here.
#   - mise: polyglot version manager (installs more toolchains at runtime)
#   - uv: Python package/project manager (+ uvx); Python via python3
#   - elixir (pulls erlang, which gleam also needs) and gleam
RUN apk add --no-cache \
      bash curl git ca-certificates \
      python3 py3-pip \
      mise \
      elixir \
      gleam
# uv isn't packaged for Alpine; copy the static musl binaries from its image.
COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /usr/local/bin/
# AI coding agents: claude, codex, gemini, opencode, kilo ship as npm CLIs. (amp is
# glibc-only — its native binary aborts even under gcompat — so it is only in
# the Debian/Ubuntu images.) libgcc/libstdc++ + a system ripgrep cover the
# glibc-linked helpers (USE_BUILTIN_RIPGREP=0 points claude at the system rg).
ENV USE_BUILTIN_RIPGREP=0
RUN apk add --no-cache nodejs npm libgcc libstdc++ ripgrep \
    && npm install -g --no-fund --no-audit \
      @anthropic-ai/claude-code \
      @openai/codex \
      @google/gemini-cli \
      opencode-ai \
      @kilocode/cli \
    && npm cache clean --force
# Kiro CLI is not on npm; its release zip has musl builds that run on Alpine.
RUN apk add --no-cache unzip \
    && curl -fsSL "https://prod.download.cli.kiro.dev/stable/latest/kirocli-$(uname -m)-linux-musl.zip" \
      -o /tmp/kirocli.zip \
    && unzip -q /tmp/kirocli.zip -d /tmp \
    && install -m755 /tmp/kirocli/bin/kiro-cli /tmp/kirocli/bin/kiro-cli-chat \
      /tmp/kirocli/bin/kiro-cli-term /usr/local/bin/ \
    && rm -rf /tmp/kirocli /tmp/kirocli.zip \
    && apk del unzip
COPY --from=build /src/target/release/cvisor /usr/local/bin/cvisor
COPY --from=build /src/target/release/cvisord /usr/local/bin/cvisord
# gRPC and GraphQL, respectively (the daemon binds 0.0.0.0 by default).
EXPOSE 50051 8080
ENTRYPOINT ["cvisor"]
