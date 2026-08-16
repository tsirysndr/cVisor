# cVisor — Ruby SDK

A `fiddle` FFI wrapper over the `libcvisor` C ABI. Linux-only.

## Usage

```ruby
require "cvisor"

sb = Cvisor::Sandbox.new
out = sb.run("echo hello")
puts out.stdout    # "hello\n"
puts out.stderr    # ""
puts out.exit_code # 0
```

`Sandbox#run(cmd, timeout_ms: nil)` blocks until the sandboxed command exits and
returns an `Output` with `#stdout` / `#stderr` (String), `#stdout_bytes` /
`#stderr_bytes`, and `#exit_code` (Integer, shell convention: status, or
128+signo when killed by a signal).

Pass a positive `timeout_ms:` to SIGKILL the guest after that many
milliseconds; a timed-out run reports exit code 137:

```ruby
sb.run("sleep 30", timeout_ms: 300).exit_code  # 137
```

`Sandbox#set_allow_network(allow)` toggles outbound INET/INET6 networking for
the sandbox (allowed by default):

```ruby
sb.set_allow_network(false)  # deny outbound networking
```

`Sandbox#set_allow_listen(allow)` toggles inbound TCP servers (listening
sockets) inside the sandbox (denied by default):

```ruby
sb.set_allow_listen(true)  # allow inbound TCP servers
sb.set_env("TOKEN", "xyz")  # environment variable for the guest
```

## Files

`Sandbox#write_file(path, data)` seeds a file into the sandbox's persistent
overlay at an absolute `path`; it is visible to later `#run` calls of the same
`Sandbox` instance. `Sandbox#read_file(path)` returns the guest's view of an
absolute `path` (the overlay copy if present, else the real host file for cow
paths) as a binary `String` (`""` for an empty or missing file). Paths must be
absolute; `/proc` and passthrough paths are not writable.

```ruby
sb.write_file("/tmp/data.txt", "seeded\n")
sb.run("grep seeded /tmp/data.txt").stdout  # "seeded\n"

sb.run("echo from-run > /tmp/out.txt")
sb.read_file("/tmp/out.txt")                # "from-run\n"
```

## Copying files and directories

`Sandbox#copy_into(host_path, guest_path)` copies a host file or directory tree
into the sandbox overlay at `guest_path`, visible to later `#run` calls.
Directory copies are recursive and honor `.gitignore`/`.dockerignore`.
`Sandbox#copy_out(guest_path, host_path)` copies the guest's view of
`guest_path` (file or directory) back out to the host. Both raise on error.

```ruby
sb.copy_into("./project", "/work")
sb.run("ls /work").stdout
sb.copy_out("/work/build", "./build")
```

## Cache

`Sandbox#cache_save(sandbox_path, key, backend: "", format: "gzip")` archives
the sandbox directory `sandbox_path` under `key` in a cache backend.
`Sandbox#cache_restore(sandbox_path, key, backend: "", format: "gzip")` unpacks
a stored archive back into the sandbox overlay at `sandbox_path`. Both raise on
error. Directory archives honor `.gitignore`/`.dockerignore`.

```ruby
sb.cache_save("/work/node_modules", "deps")
# ... later, in a fresh sandbox ...
sb2.cache_restore("/work/node_modules", "deps")
```

- `backend`: `""` or `"disk"` (default), `"disk:/path"` for a specific
  directory, or an `"s3://bucket/prefix?..."` URL. **S3 requires `libcvisor`
  built with the `s3` feature; the bundled library is disk-only.**
- `format`: `"gzip"` (default), `"estargz"`, `"none"`, or `"zstd"`. **`zstd`
  requires `libcvisor` built with that feature; the bundled library does not
  include it.**

## Streaming sessions

`Sandbox#run_streaming(command, on_stdout:, on_stderr:, poll_ms: 15)` starts a
non-PTY session and streams output to the callbacks (each receives a UTF-8
`String`) as it arrives, blocking until the command exits and returning its
exit code:

```ruby
code = sb.run_streaming("for i in 1 2 3; do echo line$i; sleep 0.1; done",
                        on_stdout: ->(s) { print s },
                        on_stderr: ->(s) { warn s })
puts code # 0
```

## Interactive PTY shell

`Sandbox#shell(on_output: nil, poll_ms: 15)` starts an interactive PTY session
(`/bin/sh -i`) and returns a `Cvisor::Session`. A PTY merges stdout and stderr,
so all output arrives via `#read_stdout` (and the `on_output:` callback):

```ruby
buf = []
sh = sb.shell(on_output: ->(s) { buf << s })
sh.write_stdin("echo hello\n")   # bytes written (Integer)
sh.resize(40, 120)               # rows, cols
sh.write_stdin("exit 0\n")
code = sh.wait                   # blocks -> exit code
sh.close                         # free the session (idempotent)
puts buf.join
```

`Cvisor::Session` methods:

- `#read_stdout` / `#read_stderr` -> `String`: drain any new bytes (`""` when none). PTY sessions deliver everything on stdout.
- `#write_stdin(data)` -> `Integer`: write `data` (a `String`) to the PTY, returns bytes written (PTY sessions only).
- `#resize(rows, cols)`: resize the PTY window.
- `#exit_code` -> `Integer` or `nil`: non-blocking; the exit code once finished, else `nil`.
- `#wait` -> `Integer`: block until exit, returning the exit code.
- `#kill`: SIGKILL the session's process.
- `#close`: free the session (idempotent).

## Remote daemon (GraphQL) — works on macOS

The `Sandbox` above needs the native `libcvisor` (Linux only). To use cVisor
from **any** platform, including macOS, talk to a running
[`cvisord`](../../crates/cvisor-daemon) over its GraphQL API. This path is pure
stdlib (`net/http` + `json`) — no native library — and `require "cvisor"` never
loads the `.so`:

```ruby
require "cvisor"

remote = Cvisor::RemoteSandbox.new("http://127.0.0.1:8080/graphql", token)
out = remote.run("echo hello")
puts out["stdout"]                          # "hello\n"

sb = remote.create_sandbox("my-box")
remote.write_file(sb["id"], "/app/x", "hi") # base64 handled for you
remote.read_file(sb["id"], "/app/x")        # "hi"
snap = remote.snapshot(sb["id"])
remote.fork(sb["id"], "clone")
remote.free_sandbox(sb["id"])
```

`RemoteSandbox` mirrors the daemon surface: `#run`, `#create_sandbox` /
`#list_sandboxes` / `#free_sandbox` / `#configure`, `#write_file` / `#read_file`,
`#cache_save` / `#cache_restore` / `#cache_list`, `#snapshot` / `#rollback` /
`#branch` / `#fork` / `#snapshots` / `#delete_snapshot`, and `#health`. For raw
documents, drop to the client:

```ruby
gql = Cvisor::GraphQL.new("http://127.0.0.1:8080/graphql", token)
data = gql.query("{ sandboxes { id name } }")
gql.mutate("mutation($c:String!){ run(command:$c){ stdout } }", { "c" => "uname -a" })
```

The daemon prints its bearer `token` on startup (or set `CVISOR_TOKEN`). On a
non-Linux host, constructing the FFI `Cvisor::Sandbox` raises a clear
"Linux-only" error pointing you here.

## Interactive console

Launch an IRB session with a live sandbox preloaded:

```bash
bin/console
```

```
cVisor interactive console
  sb          -> a Sandbox instance
  sh("cmd")   -> run a shell command in the sandbox, printing stdout/stderr
  Cvisor::Sandbox.new -> create your own

irb(main):001> sh("echo hello; uname -n")
hello
cvisor
```

## Development

The SDK loads `libcvisor.so`. Build it from the repo root (`cargo xtask ffi`),
which drops a copy into `native/`, or point the SDK at one via the `CVISOR_LIB`
environment variable.
