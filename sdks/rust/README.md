# cVisor Rust SDK

A Rust client for the [cVisor](../../README.md) sandbox runtime.

The crate is named `cvisor` and is a **standalone workspace** — it is not part of
the repo's main Cargo workspace (like `ui/src-tauri`), so it never disturbs the
FFI cross-builds.

Two layers:

| Layer                       | Transport                        | Platforms               |
| --------------------------- | -------------------------------- | ----------------------- |
| `Client` / `RemoteSandbox`  | daemon GraphQL over HTTP (`ureq`) | any (macOS, Linux, ...) |
| `Sandbox`                   | in-process `cvisor-core`         | Linux only              |

The GraphQL path is a small blocking client (`ureq` + `serde`/`serde_json`, no
async). It is the primary API and works on macOS. The native `Sandbox` and its
`cvisor-core` dependency are gated behind `#[cfg(target_os = "linux")]`, so
`cargo build` succeeds on macOS with only the remote path compiled.

## Remote: talk to a daemon (any OS)

```rust
use cvisor::{RemoteSandbox, RunOptions};

fn main() -> Result<(), cvisor::Error> {
    let mut sb = RemoteSandbox::new("http://127.0.0.1:8080/graphql", "my-token");

    let health = sb.health()?;
    println!("daemon {} ok={}", health.version, health.ok);

    // One-shot run in an ephemeral sandbox.
    let out = sb.run("echo hi", RunOptions::default())?;
    print!("{}", out.stdout); // "hi\n"
    println!("exit {}", out.exit_code);

    // Persistent sandbox + file seed.
    let info = sb.create_sandbox("")?; // random name; binds this handle to it
    println!("created {} ({})", info.name, info.id);
    sb.write_file("/work/hello.txt", b"hi\n")?;
    let out = sb.run("cat /work/hello.txt", RunOptions::default())?;
    print!("{}", out.stdout);
    let data = sb.read_file("/work/hello.txt")?;
    assert_eq!(data, b"hi\n");

    Ok(())
}
```

`RemoteSandbox` mirrors the daemon operations: `health`, `create_sandbox`,
`list_sandboxes`, `free_sandbox`, `configure`, `run`, `snapshot`, `rollback`,
`branch`, `fork`, `snapshots`, `delete_snapshot`, `write_file`, `read_file`,
`cache_save`, `cache_restore`, `cache_list`. Optionals use `Option`/`Default`
(`RunOptions`, `ConfigureOptions`, `Limits`); base64 payloads are handled
internally. For raw access, use the client directly:
`sb.client().query(doc, serde_json::json!({ ... }))`.

## Native: in-process sandbox (Linux only)

On Linux you can run the sandbox in-process, with no daemon:

```rust
# #[cfg(target_os = "linux")]
# fn main() -> Result<(), cvisor::Error> {
use cvisor::Sandbox;

let mut sb = Sandbox::new();
sb.set_allow_network(false).set_env("FOO", "bar");
sb.write_file("/work/x", b"hello")?;
let out = sb.run("cat /work/x")?;
print!("{}", out.stdout); // "hello"
# Ok(())
# }
```

`Sandbox` (`set_allow_network`, `set_allow_listen`, `set_log_debug`, `set_env`,
`set_limits`, `write_file`, `read_file`, `run`, `run_timeout`) exists **only** on
Linux — its declaration and the `cvisor-core` dependency are
`#[cfg(target_os = "linux")]`-gated. On macOS/Windows the type is not compiled at
all; use `RemoteSandbox`.

## Build

```bash
cargo build          # macOS: compiles the GraphQL path only (Sandbox cfg'd out)
```

On Linux the full crate (including `Sandbox`) builds. Cross-compiling the remote
path to a musl target from macOS requires a musl C toolchain because `ureq`'s
default TLS backend (`ring`) has C/asm — build natively on Linux (or in a
container) for the native `Sandbox`.
