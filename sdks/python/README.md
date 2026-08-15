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
