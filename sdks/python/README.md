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
`Output` with `.stdout` / `.stderr` (str) and `.stdout_bytes` / `.stderr_bytes`.

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
