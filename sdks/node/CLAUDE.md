# Node SDK

## Layout

```
sdks/node/
  index.ts              # Package entry point: re-exports Sandbox, sh, Output
  src/
    native.ts           # FFI contract: NativeModule interface, platform check, require()
    napi.ts             # External<T> phantom type for opaque native handles
    sandbox.ts          # Sandbox class + `sh` tagged-template runner (public API)
  test.ts               # e2e smoke test (run via `cargo xtask run-node`)
  examples/             # runnable examples (cargo xtask run-node --script examples/…)
  platforms/
    linux-{arm64,x64}-{gnu,musl}/   # per-platform npm packages holding libcvisor.node
```

The native binding is implemented in **Rust** (crate `cvisor-node` at the repo
root, napi-rs), producing `libcvisor.node`. Build/run via
`cargo xtask run-node` / `cargo xtask node-artifacts`.

## FFI boundary

`src/native.ts` is the single source of truth for the native contract:

```ts
createSandbox(): External<"Sandbox">;
sandboxSetLogLevel(sandbox, "OFF" | "DEBUG"): void;
sandboxRunCmd(sandbox, command): { stdout: External<"Stream">, stderr: External<"Stream"> };
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
