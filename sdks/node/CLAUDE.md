# Node / Bun / Deno SDK

One npm package (`cvisor`) serving three runtimes via conditional exports:
Node gets the napi entry, Bun (`"bun"` condition) and Deno (`"deno"`
condition) get FFI entries over the libcvisor C ABI. All three expose the
same `Sandbox` / `sh` / `Output` surface — keep them in sync.

## Layout

```
sdks/node/
  index.ts              # napi entry point: re-exports Sandbox, sh, Output
  src/
    native.ts           # napi contract: NativeModule interface, platform check, require()
    napi.ts             # External<T> phantom type for opaque native handles
    sandbox.ts          # napi Sandbox + `sh` tagged-template runner
    output.ts           # shared Output shape + createOutput/bytesToStream/buildCommand
    libpath.ts          # locates libcvisor.so (CVISOR_LIB, else platform package)
    bun.ts              # "bun" condition: bun:ffi over libcvisor.so
    deno.ts             # "deno" condition: Deno.dlopen over libcvisor.so
  test.ts               # napi e2e smoke test (run via `cargo xtask run-node`)
  test-bun.ts           # Bun-entry e2e (CI: oven/bun:alpine + CVISOR_LIB)
  test-deno.ts          # Deno-entry e2e (CI: denoland/deno:alpine + CVISOR_LIB)
  examples/             # runnable examples (cargo xtask run-node --script examples/…)
  platforms/
    linux-{arm64,x64}-{gnu,musl}/   # per-platform npm packages: libcvisor.node + libcvisor.so
```

The native binding is implemented in **Rust** (crate `cvisor-node` at the repo
root, napi-rs), producing `libcvisor.node`. Build/run via
`cargo xtask run-node` / `cargo xtask node-artifacts`.

## FFI boundary

`src/native.ts` is the single source of truth for the native contract:

```ts
createSandbox(): External<"Sandbox">;
sandboxSetLogLevel(sandbox, "OFF" | "DEBUG"): void;
sandboxSetAllowNetwork(sandbox, allow: boolean): void;
sandboxRunCmd(sandbox, command, timeoutMs?): { stdout: External<"Stream">, stderr: External<"Stream">, exitCode: number };
streamNext(stream): Uint8Array | null;
```

When adding a native function: add it to the `NativeModule` interface in
`native.ts`, then implement it in `crates/cvisor-node/src/lib.rs` with a
matching `#[napi(js_name = "...")]`. Opaque handles use `External<T>`
(`napi.ts`) on the TS side; the Rust side returns `napi::External<T>` and
receives `ExternalRef<T>`.

## Platform packages

The `platforms/` subdirectories are npm workspace packages. `npm install` on
Linux resolves them locally via workspaces. On macOS `npm install` skips them
(`os`/`cpu` filtering), so build and test in Docker via `cargo xtask run-node`.
