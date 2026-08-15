# Alpine image providing the `cvisor` CLI.
#
# Build:  docker build -t cvisor .
# Run:    docker run --rm -it --security-opt seccomp=unconfined cvisor            # interactive shell
#         docker run --rm --security-opt seccomp=unconfined cvisor -- uname -a    # run a command
#
# cVisor installs its own seccomp filter, so the container must run with the
# default seccomp profile disabled (--security-opt seccomp=unconfined).

FROM rust:alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
# rust:alpine targets musl natively, so this links a static, self-contained
# binary. Only the CLI (and its cvisor-core dep) is built.
RUN cargo build -p cvisor-core --bin cvisor --release

FROM alpine:3.20
# Tools available to sandboxed commands (busybox provides /bin/sh, which the
# CLI execs). Add anything your workloads need here.
COPY --from=build /src/target/release/cvisor /usr/local/bin/cvisor
ENTRYPOINT ["cvisor"]
