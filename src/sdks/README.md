# bVisor SDKs

bVisor ships one **native runtime** and several thin language SDKs that wrap it.

| SDK    | Mechanism                                     | Entry point          |
| ------ | --------------------------------------------- | -------------------- |
| Node   | N-API native module (`libbvisor.node`)        | `node/`              |
| Bun    | Bun FFI (`bun:ffi`) over `libbvisor.so`       | `bun/index.ts`       |
| Deno   | Deno FFI (`Deno.dlopen`) over `libbvisor.so`  | `deno/mod.ts`        |
| Python | `ctypes` FFI over `libbvisor.so` (uv project) | `python/`            |
| Ruby   | `fiddle` FFI over `libbvisor.so`              | `ruby/lib/bvisor.rb` |
| Erlang | NIF bridging to `libbvisor.so`                | `erlang/`            |

## The shared C ABI

Every SDK except Node loads the same `libbvisor.so` (crate `bvisor-ffi`),
which exposes a small C ABI:

```c
BvisorSandbox* bvisor_sandbox_new(void);
void           bvisor_sandbox_free(BvisorSandbox*);
void           bvisor_sandbox_set_log_level(BvisorSandbox*, int level); // 0=off 1=debug
BvisorOutput*  bvisor_run(BvisorSandbox*, const char* cmd);             // blocks
void           bvisor_output_free(BvisorOutput*);
uint8_t*       bvisor_output_stdout(BvisorOutput*, size_t* out_len);
uint8_t*       bvisor_output_stderr(BvisorOutput*, size_t* out_len);
void           bvisor_bytes_free(uint8_t* ptr, size_t len);
```

## Building the native library

From the repo root:

```bash
cargo xtask ffi                 # builds libbvisor.so and copies it into each SDK
cargo xtask ffi --arch x86_64   # for x86_64 targets
```

The build cross-compiles a musl `cdylib` with `cargo-zigbuild`, then patches its
`NEEDED` entry to the musl runtime soname so it loads on any musl runtime
(including minimal images without `musl-dev`).

## Usage at a glance

```python
# Python (uv add bvisor)
from bvisor import Sandbox
print(Sandbox().run("echo hi").stdout)          # "hi\n"
```
```ts
// Bun / Deno
import { Sandbox } from "./index.ts";           // or ./mod.ts for Deno
console.log(new Sandbox().run("echo hi").stdout);
```
```ruby
# Ruby
require "bvisor"
puts Bvisor::Sandbox.new.run("echo hi").stdout
```
```erlang
%% Erlang
{ok, Out, _Err} = bvisor:run(<<"echo hi">>).
```
