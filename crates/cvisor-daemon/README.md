# cvisor-daemon (`cvisord`)

A network daemon exposing the full cVisor runtime over **gRPC** and a
**GraphQL** API (actix-web + async-graphql). Linux-only.

```bash
cvisord                                   # gRPC :50051, GraphQL http://…:8080/graphql
cvisord --grpc-addr 0.0.0.0:50051 --http-addr 0.0.0.0:8080
```

## Auth

Both front-ends require a bearer token in the `authorization` header
(`Bearer <token>`). The token is taken from `CVISOR_TOKEN`; if unset, one is
generated and **printed to the console on startup**:

```
cVisor daemon (cvisord) v0.1.0
  gRPC:     0.0.0.0:50051
  GraphQL:  http://0.0.0.0:8080/graphql
  token:    9f2c…              (auto-generated; set CVISOR_TOKEN to choose your own)
```

- gRPC clients send `authorization: Bearer <token>` metadata.
- GraphQL over HTTP sends the `Authorization` header; subscriptions send the
  token in the WebSocket `connection_init` payload.

## Sandboxes

Sandboxes have a Docker-style short **id** (12 hex chars) and a human-readable
**name** (`nervous_einstein`); a colliding name gets a numeric suffix. Any RPC
takes a sandbox ref (id or name); an empty ref runs in a fresh **ephemeral**
sandbox.

## Surface

The full core is exposed over both transports:

- **Sandboxes** — create / list / free / configure (network, listen, limits, env).
- **Run** — unary `Run`, or `RunStream` (gRPC server-streaming output). GraphQL
  `run` mutation.
- **Interactive PTY** — gRPC bidirectional `Shell` (client sends stdin/resize,
  server streams merged output); GraphQL `startSession` + `sessionOutput`
  subscription + `writeSession`/`resizeSession`/`killSession` mutations.
- **Files** — write / read / copy in / copy out.
- **Cache** — save / restore / list / remove / clear.
- **Health**.

The gRPC contract is `crates/cvisor-proto/proto/cvisor.proto`. GraphiQL is served
on `GET /graphql`.

## CLI as a client

The `cvisor` CLI talks to a daemon when `--remote <addr>` is passed or
`CVISOR_REMOTE` is set (with `CVISOR_TOKEN`):

```bash
export CVISOR_REMOTE=host:50051 CVISOR_TOKEN=…
cvisor -- uname -a          # runs on the daemon, streams output
cvisor                      # interactive PTY shell on the daemon
cvisor --sandbox nervous_einstein -- ls /app
```

Without a remote, the CLI runs the sandbox locally as before.

## Build

Linux-only (pulls the async server stack; on other hosts `cvisord` is a stub).
The gRPC codegen uses a vendored `protoc` (no system protobuf needed).

```bash
cargo build -p cvisor-daemon --release
cargo build -p cvisor-daemon --release --features zstd,s3   # + zstd / S3 cache
```
