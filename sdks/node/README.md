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
  exitCode: number; // shell convention: status, or 128 + signal
}
```

### Exit codes, timeouts, and network policy

```ts
const { exitCode } = sb.runCmd("exit 3");        // 3
sb.runCmd("false").exitCode;                       // 1

// SIGKILL the guest after a deadline; a timed-out run reports 137 (128 + 9):
sb.runCmd("sleep 60", { timeoutMs: 500 }).exitCode; // 137

// Egress kill switch — deny outbound INET/INET6 sockets (default allowed):
sb.setAllowNetwork(false);

// Inbound TCP servers (bind fixed port, listen) — off by default:
sb.setAllowListen(true);

// Environment variables for the guest:
sb.setEnv("TOKEN", "xyz");
```

### Files

Transfer files in and out of the sandbox overlay; a file written this way is
visible to later runs of the same sandbox:

```ts
sb.writeFile("/app/config.json", '{"k":1}');
await sb.runCmd("cat /app/config.json").stdout();
new TextDecoder().decode(sb.readFile("/tmp/result.txt"));

// Recursive directory copy (respects .gitignore / .dockerignore):
sb.copyInto("./src", "/app");
sb.copyOut("/app/dist", "./dist");
```

### Cache

Back up and restore a sandbox directory, keyed — for build caches, deps, etc.
The bundled native library uses the host disk with gzip/estargz/none; S3 and
zstd need a library built with those features.

```ts
sb.cacheSave("/app/node_modules", "deps-v1");
// ...later, in another sandbox — exact key or newest matching prefix:
sb.cacheRestore("/app/node_modules", "deps-v1");
// options: { backend: "s3://bucket/prefix", format: "estargz" }
```

Filesystem operations are virtualized (a copy-on-write overlay), and unsafe
commands are blocked:

```ts
await sb.sh`echo hi > /tmp/test.txt`.stdout(); // only visible in this sandbox
await sb.sh`chroot /tmp`.stderr();             // blocked
```

## Remote daemon (GraphQL) — works on macOS

The `Sandbox` above needs the native `libcvisor` (Linux only). To use cVisor
from **any** platform, including macOS, talk to a running
[`cvisord`](../../crates/cvisor-daemon) over its GraphQL API. This path is pure
`fetch` + `JSON` — no native library — and importing it never loads the `.so`:

```ts
import { RemoteSandbox, GraphQLClient } from "cvisor";

const remote = new RemoteSandbox("http://127.0.0.1:8080/graphql", token);
const out = await remote.run("echo hello");   // { stdout: "hello\n", stderr: "", exitCode: 0 }

const sb = await remote.createSandbox("my-box");
await remote.writeFile(sb.id, "/app/x", "hi");        // base64 handled for you
new TextDecoder().decode(await remote.readFile(sb.id, "/app/x"));  // "hi"
const snap = await remote.snapshot(sb.id);
await remote.fork(sb.id, "clone");
await remote.freeSandbox(sb.id);
```

`RemoteSandbox` mirrors the daemon surface: `run`, `createSandbox` /
`listSandboxes` / `freeSandbox` / `configure`, `writeFile` / `readFile` /
`copyInto` / `copyOut`, `cacheSave` / `cacheRestore` / `cacheList`, `snapshot` /
`rollback` / `branch` / `fork` / `snapshots` / `deleteSnapshot`, and `health`.
For raw documents, drop to the client:

```ts
const gql = new GraphQLClient("http://127.0.0.1:8080/graphql", token);
const { sandboxes } = await gql.query(`{ sandboxes { id name } }`);
await gql.mutate(`mutation($c:String!){ run(command:$c){ stdout } }`, { c: "uname -a" });
```

The daemon prints its bearer `token` on startup (or set `CVISOR_TOKEN`). On a
non-Linux host, constructing the FFI `Sandbox` throws a clear "Linux-only" error
pointing you here.

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
