# cVisor — Node / Bun / Deno SDK

One npm package for all three JS runtimes. Under Node it binds via a native
N-API module (`libcvisor.node`); under Bun and Deno the package's `"bun"` and
`"deno"` export conditions select FFI entries (`bun:ffi` / `Deno.dlopen`) over
`libcvisor.so`. All entries expose the same `Sandbox` / `sh` / `Output` API.
Linux only (ARM & x86, glibc & musl).

## Install

```bash
npm install cvisor        # Node
bun add cvisor            # Bun
deno add npm:cvisor       # Deno (run with --allow-ffi)
```

Under Deno the sandbox needs FFI permission: `deno run --allow-ffi ...`
(plus `--allow-env` if you use the `CVISOR_LIB` override). The runtime is
picked automatically; `cvisor/bun` and `cvisor/deno` are also exposed as
explicit subpaths.

## Usage

```ts
import { Sandbox, sh } from "cvisor";

// Explicit sandbox — filesystem writes are isolated per sandbox.
const sb = new Sandbox();
const out = sb.runCmd("echo 'Hello, world!'");
console.log(await out.stdout()); // "Hello, world!\n"

// Tagged-template runner on a sandbox:
console.log(await sb.sh`uname -n`.stdout()); // "cvisor\n"

// Or the standalone `sh`, which uses a shared, lazily-created sandbox:
const files = await sh`ls -l ${"/tmp"}`.stdout();
```

`runCmd(cmd)` / `sh\`…\`` block until the command exits and return an `Output`:

```ts
interface Output {
  stdout: () => Promise<string>;
  stderr: () => Promise<string>;
  stdoutStream: ReadableStream<Uint8Array>;
  stderrStream: ReadableStream<Uint8Array>;
}
```

Filesystem operations are virtualized (a copy-on-write overlay), and unsafe
commands are blocked:

```ts
await sb.sh`echo hi > /tmp/test.txt`.stdout(); // only visible in this sandbox
await sb.sh`chroot /tmp`.stderr();             // blocked
```

## Development

The native runtime is the Rust workspace at the repo root (crate
`cvisor-node`), built with `cargo-zigbuild`. From the repo root:

```bash
cargo xtask run-node                 # build libcvisor.node + run test.ts in a bun container
cargo xtask run-node --script examples/hello-world.ts
cargo xtask node-artifacts           # build libcvisor.node for all 4 platform packages
```

`libcvisor.node` and `libcvisor.so` are produced per platform into
`platforms/linux-<arch>-<libc>/`. The napi loader (`src/native.ts`) and the
FFI loader (`src/libpath.ts`) resolve the right one at runtime via
`detect-libc`; `CVISOR_LIB` overrides the `.so` path. On macOS, `npm install`
skips the platform packages (`os`/`cpu` filtering), so use the Docker flow
above. The Bun/Deno e2e tests are `test-bun.ts` / `test-deno.ts` (run by CI in
Alpine containers with `CVISOR_LIB` set).

## Publishing

Bump versions across all packages, then publish:

```bash
bun run version:patch
bun run publish:all      # builds the .node artifacts (cargo xtask node-artifacts) then publishes
```
