# cVisor SDKs

cVisor ships one **native runtime** and several thin language SDKs that wrap it.

| SDK     | Mechanism                                              | Entry point          |
| ------- | ------------------------------------------------------ | -------------------- |
| Node    | N-API native module (`libcvisor.node`)                 | `node/`              |
| Bun     | Bun FFI (`bun:ffi`) over `libcvisor.so`                | `node/src/bun.ts`    |
| Deno    | Deno FFI (`Deno.dlopen`) over `libcvisor.so`           | `node/src/deno.ts`   |
| Python  | `ctypes` FFI over `libcvisor.so` (uv project)          | `python/`            |
| Ruby    | `fiddle` FFI over `libcvisor.so`                       | `ruby/lib/cvisor.rb` |
| Erlang  | NIF bridging to `libcvisor.so`                         | `erlang/`            |
| Elixir  | pipe-friendly builder over the Erlang `cvisor` NIF     | `elixir/`            |
| Gleam   | pipe-friendly builder over the Erlang `cvisor` NIF     | `gleam/`             |
| Clojure | Java FFM (`java.lang.foreign`) over `libcvisor.so`     | `clojure/`           |
| Go      | daemon GraphQL (any OS) + cgo `libcvisor` (Linux)      | `go/`                |
| Rust    | daemon GraphQL (any OS) + `cvisor-core` (Linux)        | `rust/`              |
| Scala   | daemon GraphQL (any OS) + Java FFM `libcvisor` (Linux) | `scala/`             |

The Elixir (`cvisor_ex`) and Gleam (`gleam_cvisor`) SDKs both run on the BEAM
and reuse the Erlang SDK's NIF runtime (a path dependency on `erlang/`), adding
an idiomatic immutable builder you compose with the pipe operator `|>`.

The Go (`github.com/tsirysndr/cvisor-go`) and Rust (`cvisor`) SDKs are
**daemon-first**: their primary API is a portable client for the daemon's GraphQL
HTTP endpoint (works on macOS with no native library), with the in-process native
runtime available only on Linux (Go via cgo over `libcvisor`, Rust via a direct
`cvisor-core` dependency). See `go/README.md` and `rust/README.md`.

Node, Bun, and Deno all install the same `cvisor` npm package: its `exports`
map picks the napi entry under Node and the FFI entries under Bun (`"bun"`
condition) and Deno (`"deno"` condition), all exposing the same
`Sandbox` / `sh` / `Output` API. The FFI entries load `libcvisor.so` from the
`@cvisor/linux-*` platform packages (or `CVISOR_LIB`).

## The shared C ABI

Every SDK except Node loads the same `libcvisor.so` (crate `cvisor-ffi`),
which exposes a small C ABI:

```c
CvisorSandbox* cvisor_sandbox_new(void);
void           cvisor_sandbox_free(CvisorSandbox*);
void           cvisor_sandbox_set_log_level(CvisorSandbox*, int level);      // 0=off 1=debug
void           cvisor_sandbox_set_allow_network(CvisorSandbox*, int allow);  // 0=deny else allow
CvisorOutput*  cvisor_run(CvisorSandbox*, const char* cmd);                  // blocks
CvisorOutput*  cvisor_run_timeout(CvisorSandbox*, const char* cmd, uint64_t timeout_ms);
int            cvisor_output_exit_code(CvisorOutput*);                       // status, or 128+signo
void           cvisor_output_free(CvisorOutput*);
uint8_t*       cvisor_output_stdout(CvisorOutput*, size_t* out_len);
uint8_t*       cvisor_output_stderr(CvisorOutput*, size_t* out_len);
void           cvisor_bytes_free(uint8_t* ptr, size_t len);
```

`cvisor_run_timeout` SIGKILLs the guest process group when `timeout_ms` elapses
(0 = no limit); a timed-out run reports exit code 137 (128 + SIGKILL). With
networking disabled the guest cannot open INET/INET6 sockets;
`cvisor_sandbox_set_allow_listen` opts into inbound TCP servers (bind a fixed
port, `listen`, `accept`). `cvisor_sandbox_set_env(sb, key, value)` sets a guest
environment variable (layered over the default PATH/HOME) — each SDK wraps it as
`set_env`.

Files can be transferred in and out of a sandbox's persistent overlay without a
running guest — `cvisor_sandbox_write_file` / `cvisor_sandbox_read_file` (single
file), and `cvisor_sandbox_copy_into` / `cvisor_sandbox_copy_out` (files or whole
directory trees, recursive and `.gitignore`/`.dockerignore`-aware). A file
written this way is visible to a later `cvisor_run` of the same sandbox.

`cvisor_cache_save` / `cvisor_cache_restore` back up and restore a sandbox
directory as a keyed archive — `backend` is the host disk by default (or
`s3://bucket/prefix`), `format` is gzip / estargz / zstd / none, and restore
takes an exact key or the newest key with that prefix. Each SDK wraps these as
`copy_into` / `copy_out` / `cache_save` / `cache_restore` (plus `set_allow_listen`).
The prebuilt `libcvisor.so` shipped with the SDKs is pure-Rust (disk backend +
gzip/estargz/none); the S3 backend and zstd need a library built with those
cargo features.

For streaming output and interactive shells, the ABI also exposes **sessions**
(`cvisor_session_*`): the guest runs in the background while the caller drains
`read_stdout`/`read_stderr` as output arrives, feeds `write_stdin` (PTY mode),
`resize`s, and `wait`s. `cvisor_session_start(sb, cmd, pty)` with `pty=1` runs
`/bin/sh -i` on a pseudo-terminal (merged output; stdin writable). Each SDK
builds idiomatic stdout/stderr callbacks on top of the drain functions.

## Building the native library

From the repo root:

```bash
cargo xtask ffi                 # builds libcvisor.so and copies it into each SDK
cargo xtask ffi --arch x86_64   # for x86_64 targets
```

The default build cross-compiles a **pure-Rust** musl `cdylib` with
`cargo-zigbuild`, then patches its `NEEDED` entry to the musl runtime soname so
it loads on any musl runtime (including minimal images without `musl-dev`). This
build supports the disk cache backend and the gzip/estargz/none archive formats.

### All features (zstd + S3)

The zstd format and the S3 cache backend need C dependencies (`zstd-sys`, and
`ring` for S3 TLS) that don't cross-compile under `cargo-zigbuild`. To ensure
`libcvisor.so` has **all** features, build it natively inside `rust:alpine`:

```bash
cargo xtask ffi --all-features                 # native arch
cargo xtask ffi --all-features --arch x86_64   # other arch (via qemu)
```

This forwards the `zstd`/`s3` cargo features (declared in `cvisor-ffi` →
`cvisor-core`) and builds with a native musl toolchain. The resulting `.so` has
one extra runtime dependency, `libgcc_s.so.1` (ubiquitous — present on glibc,
`apk add libgcc` on Alpine); the default pure-Rust `.so` has none.

## Usage at a glance

```python
# Python (uv add cvisor)
from cvisor import Sandbox
print(Sandbox().run("echo hi").stdout)          # "hi\n"
```
```ts
// Node / Bun (bun add cvisor) / Deno (deno add npm:cvisor)
import { Sandbox } from "cvisor";
console.log(await new Sandbox().runCmd("echo hi").stdout());
```
```ruby
# Ruby
require "cvisor"
puts Cvisor::Sandbox.new.run("echo hi").stdout
```
```erlang
%% Erlang
{ok, Out, _Err, 0} = cvisor:run(<<"echo hi">>).
```
```elixir
# Elixir (cvisor_ex on Hex) — pipe-friendly builder
Cvisor.new() |> Cvisor.memory_limit(256 * 1024 * 1024) |> Cvisor.run("echo hi")
```
```gleam
// Gleam (gleam_cvisor on Hex) — pipe-friendly builder
import gleam_cvisor as cvisor
cvisor.new() |> cvisor.pids_limit(128) |> cvisor.run("echo hi")
```
```clojure
;; Clojure (io.github.tsirysndr/cvisor on Clojars, JDK 22+)
(require '[cvisor.core :as cvisor])
(with-open [sb (cvisor/sandbox)]
  (print (:stdout (cvisor/run sb "echo hi"))))    ; "hi\n"
```

Every SDK also surfaces the run's exit code, an optional per-run timeout, and a
network on/off toggle (see each SDK's README).

## Interactive consoles

The Python, Ruby, and Clojure SDKs ship a REPL with a live sandbox (`sb`) and
an `sh("cmd")` helper preloaded:

```bash
uv run --extra console cvisor    # Python  -> IPython
bin/console                      # Ruby    -> IRB (from sdks/ruby)
clojure -M:console               # Clojure -> rebel-readline (from sdks/clojure)
```
