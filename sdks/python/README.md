# cVisor — Python SDK

A `ctypes` FFI wrapper over the `libcvisor` C ABI. Linux-only.

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

## Development

The SDK loads `libcvisor.so`. Build it from the repo root and point the SDK at
it via the `CVISOR_LIB` environment variable, or let the package resolve a copy
bundled under `cvisor/_native/`:

```bash
# from the repo root — builds libcvisor.so into cvisor/_native/
cargo xtask ffi

# run the tests with uv
cd src/sdks/python
uv run pytest
```
