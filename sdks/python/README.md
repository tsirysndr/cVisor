# cVisor — Python SDK

A `ctypes` FFI wrapper over the `libcvisor` C ABI. Linux-only.

## Quick try (Docker)

Drop into a Python REPL with cvisor installed, from any machine with Docker:

```bash
docker run -it --rm \
  --security-opt seccomp=unconfined --security-opt apparmor=unconfined \
  ghcr.io/astral-sh/uv:python3.12-alpine \
  uv run --with cvisor python
```

```python
>>> from cvisor import Sandbox
>>> sb = Sandbox()
>>> print(sb.run("echo hi; uname -n").stdout)
hi
cvisor
```

The `--security-opt` flags are required: cVisor installs its own seccomp
filter, which Docker's default profiles block. An Alpine (musl) image is
needed — the published wheels are `musllinux`-tagged.

## Install (uv)

```bash
uv add cvisor
```

## Usage

```python
from cvisor import Sandbox

with Sandbox() as sb:
    out = sb.run("echo hello")
    print(out.stdout)   # "hello\n"
    print(out.stderr)   # ""
```

`Sandbox.run(cmd)` blocks until the sandboxed command exits and returns an
`Output` with `.stdout` / `.stderr` (str), `.stdout_bytes` / `.stderr_bytes`,
and `.exit_code` (int, shell convention: the command's status, or 128+signo if
it was killed by a signal).

### Timeouts

`Sandbox.run(cmd, timeout_ms=...)` SIGKILLs the guest after `timeout_ms`
milliseconds; a timed-out run reports exit code 137:

```python
out = sb.run("sleep 30", timeout_ms=300)
assert out.exit_code == 137
```

### Network policy

`Sandbox.set_allow_network(allow)` controls outbound INET/INET6 networking
(allowed by default):

```python
sb.set_allow_network(False)  # deny outbound networking
```

`Sandbox.set_allow_listen(allow)` controls inbound TCP servers (`listen`/
`accept`), denied by default:

```python
sb.set_allow_listen(True)  # allow the guest to run TCP servers
```

### Files

`Sandbox.write_file(path, data)` writes `data` (`bytes` or `str`) into the
sandbox's persistent overlay, and `Sandbox.read_file(path)` returns the file's
bytes as the guest sees it (the overlay copy, else the real host file for cow
paths). `path` must be absolute; `/proc` and passthrough paths are not writable.
Files written are visible to later `run` calls of the same `Sandbox` instance:

```python
with Sandbox() as sb:
    sb.write_file("/tmp/data.txt", "seeded\n")
    print(sb.run("cat /tmp/data.txt").stdout)   # "seeded\n"

    sb.run("echo from-run > /tmp/out.txt")
    print(sb.read_file("/tmp/out.txt"))         # b"from-run\n"
```

`write_file` raises `OSError` on failure; `read_file` returns `b""` for an empty
file and raises `FileNotFoundError` for a missing or unreadable path.

### Copying files and directories

`Sandbox.copy_into(host_path, guest_path)` copies a host file — or a whole
directory tree — into the sandbox's persistent overlay, and
`Sandbox.copy_out(guest_path, host_path)` copies one back out to the real
filesystem. Directory copies are recursive and respect `.gitignore` /
`.dockerignore`. Copied files are visible to later `run` calls:

```python
with Sandbox() as sb:
    sb.copy_into("./my-project", "/work")     # host dir -> overlay
    sb.run("cd /work && make")
    sb.copy_out("/work/build", "./build-out")  # overlay -> host
```

Both raise `OSError` on failure.

### Cache

`Sandbox.cache_save(sandbox_path, key, backend="", format="gzip")` archives a
sandbox **directory** under `key` to a cache backend, and
`Sandbox.cache_restore(sandbox_path, key, backend="", format="gzip")` unpacks it
back into the overlay. This lets one sandbox persist a directory (a build tree,
a dependency cache, …) that a later, independent sandbox can restore:

```python
with Sandbox() as sb:
    sb.run("cd /work && npm install")
    sb.cache_save("/work/node_modules", "npm-deps")

with Sandbox() as sb2:
    sb2.cache_restore("/work/node_modules", "npm-deps")  # ready to use
```

Both the archive and `cache_save` respect `.gitignore` / `.dockerignore`.

`backend` selects where the archive is stored:

- `""` or `"disk"` — host disk (the default)
- `"disk:/path"` — host disk at an explicit location
- `"s3://bucket/prefix?region=..&endpoint=.."` — S3

S3 requires the library to be built with the `s3` feature; **the bundled
`libcvisor.so` is not, so `disk` is the working default.**

`format` selects the archive format:

- `""` or `"gzip"` — gzip (the default)
- `"estargz"` — seekable, lazily-pullable gzip
- `"none"` — uncompressed
- `"zstd"` — zstd, only if the library is built with zstd (**the bundled lib is
  not**)

Both methods raise `OSError` on failure.

### Streaming output

`Sandbox.run_streaming(cmd, on_stdout=..., on_stderr=..., poll_ms=15)` runs a
command and delivers output to callbacks as it is produced, instead of buffering
it all until exit. Each callback receives a decoded `str` chunk. Returns the
exit code:

```python
with Sandbox() as sb:
    sb.run_streaming(
        "for i in 1 2 3; do echo line$i; sleep 0.1; done",
        on_stdout=lambda s: print(s, end=""),
    )
```

### Interactive PTY shell

`Sandbox.shell(on_output=..., poll_ms=15)` starts an interactive `/bin/sh -i`
session on a PTY (so `test -t 1` is true and programs behave as if attached to a
terminal). It returns a `Session` you can drive:

```python
with Sandbox() as sb:
    sh = sb.shell(on_output=lambda s: print(s, end=""))
    sh.write_stdin("echo hello\n")
    sh.resize(40, 120)          # rows, cols
    sh.write_stdin("exit 0\n")
    code = sh.wait()            # blocks for the exit code
    sh.close()
```

`Session` methods:

- `write_stdin(data)` — feed input (`bytes` or `str`); returns bytes written (PTY only).
- `read_stdout()` / `read_stderr()` — drain and return new bytes (a PTY merges the
  streams, so stderr is empty). `on_output` above does this for you on a daemon thread.
- `resize(rows, cols)` — resize the PTY.
- `exit_code()` — the exit code if the session has finished, else `None` (non-blocking).
- `wait()` — block until exit and return the code.
- `kill()` — SIGKILL the session.
- `close()` — free the session (idempotent; also a context manager and on `__del__`).

## Interactive console

Launch an IPython REPL with a live sandbox preloaded:

```bash
uv run --extra console cvisor   # or: python -m cvisor
```

```
cVisor interactive console
  sb          -> a Sandbox instance
  sh("cmd")   -> run a shell command in the sandbox, printing stdout/stderr
  Sandbox     -> create your own: Sandbox()

In [1]: sh("echo hello; uname -n")
hello
cvisor
```

Without the `console` extra (IPython) it falls back to the stdlib REPL.

## Development

The SDK loads `libcvisor.so`. Build it from the repo root and point the SDK at
it via the `CVISOR_LIB` environment variable, or let the package resolve a copy
bundled under `cvisor/_native/`:

```bash
# from the repo root — builds libcvisor.so into cvisor/_native/
cargo xtask ffi

# run the tests with uv
cd sdks/python
uv run pytest
```
