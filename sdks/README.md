# cVisor SDKs

cVisor ships one **native runtime** and several thin language SDKs that wrap it.

| SDK     | Mechanism                                          | Entry point          |
| ------- | -------------------------------------------------- | -------------------- |
| Node    | N-API native module (`libcvisor.node`)             | `node/`              |
| Bun     | Bun FFI (`bun:ffi`) over `libcvisor.so`            | `node/src/bun.ts`    |
| Deno    | Deno FFI (`Deno.dlopen`) over `libcvisor.so`       | `node/src/deno.ts`   |
| Python  | `ctypes` FFI over `libcvisor.so` (uv project)      | `python/`            |
| Ruby    | `fiddle` FFI over `libcvisor.so`                   | `ruby/lib/cvisor.rb` |
| Erlang  | NIF bridging to `libcvisor.so`                     | `erlang/`            |
| Clojure | Java FFM (`java.lang.foreign`) over `libcvisor.so` | `clojure/`           |

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
void           cvisor_sandbox_set_log_level(CvisorSandbox*, int level); // 0=off 1=debug
CvisorOutput*  cvisor_run(CvisorSandbox*, const char* cmd);             // blocks
void           cvisor_output_free(CvisorOutput*);
uint8_t*       cvisor_output_stdout(CvisorOutput*, size_t* out_len);
uint8_t*       cvisor_output_stderr(CvisorOutput*, size_t* out_len);
void           cvisor_bytes_free(uint8_t* ptr, size_t len);
```

## Building the native library

From the repo root:

```bash
cargo xtask ffi                 # builds libcvisor.so and copies it into each SDK
cargo xtask ffi --arch x86_64   # for x86_64 targets
```

The build cross-compiles a musl `cdylib` with `cargo-zigbuild`, then patches its
`NEEDED` entry to the musl runtime soname so it loads on any musl runtime
(including minimal images without `musl-dev`).

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
{ok, Out, _Err} = cvisor:run(<<"echo hi">>).
```
```clojure
;; Clojure (io.github.tsirysndr/cvisor on Clojars, JDK 22+)
(require '[cvisor.core :as cvisor])
(with-open [sb (cvisor/sandbox)]
  (print (:stdout (cvisor/run sb "echo hi"))))    ; "hi\n"
```

## Interactive consoles

The Python, Ruby, and Clojure SDKs ship a REPL with a live sandbox (`sb`) and
an `sh("cmd")` helper preloaded:

```bash
uv run --extra console cvisor    # Python  -> IPython
bin/console                      # Ruby    -> IRB (from sdks/ruby)
clojure -M:console               # Clojure -> rebel-readline (from sdks/clojure)
```
