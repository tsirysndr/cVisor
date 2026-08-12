# bVisor — Python SDK

A `ctypes` FFI wrapper over the `libbvisor` C ABI. Linux-only.

## Install (uv)

```bash
uv add bvisor
```

## Usage

```python
from bvisor import Sandbox

with Sandbox() as sb:
    out = sb.run("echo hello")
    print(out.stdout)   # "hello\n"
    print(out.stderr)   # ""
```

`Sandbox.run(cmd)` blocks until the sandboxed command exits and returns an
`Output` with `.stdout` / `.stderr` (str) and `.stdout_bytes` / `.stderr_bytes`.

## Development

The SDK loads `libbvisor.so`. Build it from the repo root and point the SDK at
it via the `BVISOR_LIB` environment variable, or let the package resolve a copy
bundled under `bvisor/_native/`:

```bash
# from the repo root — builds libbvisor.so into bvisor/_native/
cargo xtask ffi

# run the tests with uv
cd src/sdks/python
uv run pytest
```
