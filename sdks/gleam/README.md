# cVisor — Gleam SDK

Gleam bindings for [cVisor](https://github.com/tsirysndr/cVisor), an in-process
Linux sandbox. A thin, pipe-friendly builder over the NIF-backed `cvisor`
runtime (the Erlang SDK). Erlang target only; Linux-only at runtime.

Published on Hex as [`gleam_cvisor`](https://hex.pm/packages/gleam_cvisor).

## Install

```sh
gleam add gleam_cvisor
```

The underlying `cvisor` package ships a prebuilt `libcvisor.so` and compiles a
small NIF shim at build time (a C compiler is required on install).

## Usage

A `Sandbox` is an immutable configuration you build up with the pipe operator,
then execute with `run`:

```gleam
import gleam_cvisor as cvisor

pub fn main() {
  let out =
    cvisor.new()
    |> cvisor.run("echo hello")

  // out.stdout    == "hello\n"
  // out.exit_code == 0  (shell convention: status, or 128 + signal)
}
```

### Resource limits

Cap the guest's memory, process count, and CPU via cgroup v2 — pipe them in:

```gleam
cvisor.new()
|> cvisor.memory_limit(256 * 1024 * 1024)   // 256 MiB (memory.max)
|> cvisor.pids_limit(128)                     // pids.max
|> cvisor.cpu_limit(50)                       // 50% of one core (cpu.max)
|> cvisor.run("./build.sh")
```

An over-cap allocation is OOM-killed; `pids_limit` caps the whole guest tree.
Limits require a writable cgroup v2 hierarchy — where one is unavailable they
gracefully no-op and the run proceeds unlimited.

### Environment, network, and timeout

```gleam
cvisor.new()
|> cvisor.env("TOKEN", "xyz")     // layered over the default PATH/HOME
|> cvisor.network(False)          // deny outbound INET/INET6 (default: allowed)
|> cvisor.allow_listen(True)      // permit inbound TCP servers (default: off)
|> cvisor.timeout(5000)           // SIGKILL after 5s (a timed-out run reports 137)
|> cvisor.run("curl https://example.com")
```

Each builder returns an updated `Sandbox`, so they chain; the configuration is
applied to the shared runtime when `run` executes.

## Remote daemon (GraphQL) — works on macOS

The `gleam_cvisor` runtime above needs the native `libcvisor` (Linux only). To
use cVisor from **any** platform, including macOS, talk to a running
[`cvisord`](../../crates/cvisor-daemon) over its GraphQL API. The `cvisor/remote`
and `cvisor/graphql` modules are pure HTTP + JSON (`gleam_httpc` + `gleam_json`)
— no NIF — so they work without `libcvisor`:

```gleam
import cvisor/remote
import gleam/bit_array

pub fn main() {
  let client = remote.connect("http://127.0.0.1:8080/graphql", token)
  let assert Ok(out) = remote.run(client, "echo hello", 0)
  // out.stdout == "hello\n"

  let assert Ok(sb) = remote.create_sandbox(client, "my-box")
  let assert Ok(_) =
    remote.write_file(client, sb.id, "/app/x", bit_array.from_string("hi"))
  let assert Ok(_bytes) = remote.read_file(client, sb.id, "/app/x")  // base64 decoded
  let assert Ok(_) = remote.free_sandbox(client, sb.id)
}
```

`cvisor/remote` mirrors the daemon surface: `run`, `create_sandbox` /
`list_sandboxes` / `free_sandbox` / `configure`, `write_file` / `read_file`,
`cache_save` / `cache_restore` / `cache_list`, `snapshot` / `rollback` /
`branch` / `fork` / `snapshots` / `delete_snapshot`, and `health`; each returns
a `Result`. For raw documents use `cvisor/graphql`, whose `query`/`mutate` take
a `gleam/json` value for the variables and return the `data` as a `Dynamic`:

```gleam
import cvisor/graphql
import gleam/json

let client = graphql.client("http://127.0.0.1:8080/graphql", token)
let assert Ok(_data) = graphql.query(client, "{ sandboxes { id name } }", json.object([]))
```

The daemon prints its bearer token on startup (or set `CVISOR_TOKEN`).

## Development

The SDK depends on the `cvisor` Erlang runtime. In the monorepo it is a path
dependency (`../erlang`); building it compiles the NIF and needs the prebuilt
`libcvisor.so` in the Erlang SDK's `priv/` (run `cargo xtask ffi` from the repo
root first). Point the runtime at a specific library with `CVISOR_LIB` if the
bundled one isn't found.

Run the tests in a musl Gleam container (Linux syscalls + seccomp required):

```bash
docker run --rm --security-opt seccomp=unconfined \
  -e CVISOR_LIB="$PWD/sdks/erlang/priv/libcvisor-$(uname -m).so" \
  -v "$PWD":/w -w /w/sdks/gleam ghcr.io/gleam-lang/gleam:v1.12.0-erlang-alpine \
  sh -c 'apk add --no-cache build-base >/dev/null && gleam test'
```

## Publishing

The committed `gleam.toml` uses a path dependency on `../erlang` so `gleam test`
runs against the in-repo Erlang runtime. Hex rejects path deps in a published
package, so publish via the helper, which swaps the dependency to the Hex
`cvisor` release, runs `gleam publish`, and restores the path dep afterward:

```bash
bin/publish.sh            # forwards extra args to `gleam publish`
```

Bump `HEX_REQ` in `bin/publish.sh` when targeting a new `cvisor` release.
Releases are tagged `gleam-sdk-v*` in the monorepo.
