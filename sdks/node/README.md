# cVisor — Node SDK

Node.js bindings for cVisor via a native N-API module (`libcvisor.node`).
Linux only (ARM & x86, glibc & musl).

## Install

```bash
npm install cvisor
```

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

`libcvisor.node` is produced per platform into `platforms/linux-<arch>-<libc>/`.
The TypeScript loader (`src/native.ts`) resolves the right one at runtime via
`detect-libc`. On macOS, `npm install` skips the platform packages (`os`/`cpu`
filtering), so use the Docker flow above.

## Publishing

Bump versions across all packages, then publish:

```bash
bun run version:patch
bun run publish:all      # builds the .node artifacts (cargo xtask node-artifacts) then publishes
```
